//! Time-based Unique Identifiers (TUID).
//!
//! Globally unique, time-ordered 128-bit identifiers suitable for use as
//! event IDs, link IDs, and any other context where creation-order sorting
//! is desirable without coordination.
//!
//! # Format
//!
//! A [`Tuid`] is 16 bytes: 8 bytes of big-endian nanoseconds since epoch
//! followed by 8 bytes of a per-thread monotonic counter seeded with
//! randomness. This means:
//!
//! - Raw byte comparison sorts in creation order
//! - The string representation (hex) also sorts correctly
//! - Generation is lock-free (thread-local counter, no syscall after init)
//! - No coordination needed across threads or processes
//!
//! # Namespace Prefix Convention
//!
//! Wrap [`Tuid`] in a newtype and give it a prefix for human readability:
//!
//! ```text
//! evt_182342300C5F8C327a7b4a6e5a379ac4   (event ID)
//! lnk_182342300C5F8C327a7b4a6e5a379ac4   (link ID)
//! ```
//!
//! The prefix is only part of the string representation -- storage uses
//! the raw 16 bytes.
//!
//! # Origin
//!
//! Forked from Rerun's `re_tuid` crate. Stripped of `re_byte_size` and
//! `re_log` dependencies to be fully standalone.

/// Time-based Unique Identifier.
///
/// A globally unique, time-ordered 128-bit ID. The upper 64 bits are
/// approximate nanoseconds since epoch (big-endian). The lower 64 bits
/// are a per-thread monotonic counter seeded with randomness.
///
/// Raw bytes sort in creation order. String representation (hex) also
/// sorts correctly.
#[repr(C, align(1))]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Tuid {
    /// Big-endian nanoseconds since epoch.
    time_nanos: [u8; 8],

    /// Per-thread monotonic counter, seeded randomly, big-endian.
    inc: [u8; 8],
}

// ---------------------------------------------------------------------------
// Display / Debug / FromStr
// ---------------------------------------------------------------------------

/// Upper-case hex for time half, lower-case for counter half.
/// Example: `182342300C5F8C327a7b4a6e5a379ac4`
impl std::fmt::Display for Tuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:016X}{:016x}", self.nanos_since_epoch(), self.inc())
    }
}

impl std::fmt::Debug for Tuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl std::str::FromStr for Tuid {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        u128::from_str_radix(s, 16).map(Self::from_u128)
    }
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl Tuid {
    /// All zeroes.
    pub const ZERO: Self = Self {
        time_nanos: [0; 8],
        inc: [0; 8],
    };

    /// All ones -- the maximum possible value.
    pub const MAX: Self = Self {
        time_nanos: u64::MAX.to_be_bytes(),
        inc: u64::MAX.to_be_bytes(),
    };

    /// Create a new unique [`Tuid`] based on the current time.
    ///
    /// Thread-safe, lock-free. Each thread maintains its own monotonic
    /// counter so there is no contention.
    #[expect(clippy::new_without_default)]
    #[inline]
    pub fn new() -> Self {
        use std::cell::RefCell;

        thread_local! {
            static LATEST_TUID: RefCell<Tuid> = RefCell::new(Tuid::from_nanos_and_inc(
                monotonic_nanos_since_epoch(),
                random_u64() & !(1_u64 << 63),
            ));
        }

        LATEST_TUID.with(|latest_tuid| {
            let mut latest = latest_tuid.borrow_mut();
            let new = Self::from_nanos_and_inc(monotonic_nanos_since_epoch(), latest.inc() + 1);
            debug_assert!(
                latest.nanos_since_epoch() <= new.nanos_since_epoch(),
                "Time should be monotonically increasing"
            );
            *latest = new;
            new
        })
    }

    /// Construct from explicit nanosecond timestamp and counter value.
    #[inline]
    pub fn from_nanos_and_inc(time_nanos: u64, inc: u64) -> Self {
        Self {
            time_nanos: time_nanos.to_be_bytes(),
            inc: inc.to_be_bytes(),
        }
    }

    /// Construct from a `u128` where the upper 64 bits are time and the
    /// lower 64 bits are the counter.
    #[inline]
    pub fn from_u128(id: u128) -> Self {
        Self::from_nanos_and_inc((id >> 64) as u64, (id & (!0 >> 64)) as u64)
    }

    /// Construct from raw big-endian bytes.
    #[inline]
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self::from_u128(u128::from_be_bytes(bytes))
    }
}

// ---------------------------------------------------------------------------
// Accessors
// ---------------------------------------------------------------------------

impl Tuid {
    /// Approximate nanoseconds since unix epoch (upper 64 bits).
    #[inline]
    pub fn nanos_since_epoch(&self) -> u64 {
        u64::from_be_bytes(self.time_nanos)
    }

    /// The monotonic counter part (lower 64 bits).
    #[inline]
    pub fn inc(&self) -> u64 {
        u64::from_be_bytes(self.inc)
    }

    /// The full 128-bit value.
    #[inline]
    pub fn as_u128(&self) -> u128 {
        ((self.nanos_since_epoch() as u128) << 64) | (self.inc() as u128)
    }

    /// Big-endian byte representation (sorts identically to the Tuid).
    #[inline]
    pub fn as_bytes(&self) -> [u8; 16] {
        self.as_u128().to_be_bytes()
    }

    /// The next logical Tuid (same timestamp, counter + 1).
    ///
    /// Prefer [`Tuid::new`] for generating IDs. This is for cases where
    /// you need a deterministic successor.
    #[must_use]
    #[inline]
    pub fn next(&self) -> Self {
        let Self { time_nanos, inc } = *self;
        Self {
            time_nanos,
            inc: u64::from_be_bytes(inc).wrapping_add(1).to_be_bytes(),
        }
    }

    /// Advance the counter by `n` steps.
    #[must_use]
    #[inline]
    pub fn incremented_by(&self, n: u64) -> Self {
        let Self { time_nanos, inc } = *self;
        Self {
            time_nanos,
            inc: u64::from_be_bytes(inc).wrapping_add(n).to_be_bytes(),
        }
    }

    /// Short 8-character hex suffix for logging.
    #[inline]
    pub fn short_string(&self) -> String {
        let s = self.to_string();
        s[(s.len() - 8)..].to_string()
    }
}

// ---------------------------------------------------------------------------
// Serde
// ---------------------------------------------------------------------------

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
struct TuidSerde {
    time_nanos: u64,
    inc: u64,
}

#[cfg(feature = "serde")]
impl serde::Serialize for Tuid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TuidSerde {
            time_nanos: self.nanos_since_epoch(),
            inc: self.inc(),
        }
        .serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Tuid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let TuidSerde { time_nanos, inc } = serde::Deserialize::deserialize(deserializer)?;
        Ok(Self::from_nanos_and_inc(time_nanos, inc))
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// High-precision monotonic nanosecond counter anchored to wall-clock time.
#[inline]
fn monotonic_nanos_since_epoch() -> u64 {
    use web_time::Instant;

    static START_TIME: std::sync::LazyLock<(u64, Instant)> =
        std::sync::LazyLock::new(|| (nanos_since_epoch(), Instant::now()));
    START_TIME.0 + START_TIME.1.elapsed().as_nanos() as u64
}

fn nanos_since_epoch() -> u64 {
    if let Ok(duration_since_epoch) = web_time::SystemTime::UNIX_EPOCH.elapsed() {
        let mut nanos = duration_since_epoch.as_nanos() as u64;
        if cfg!(target_arch = "wasm32") {
            nanos += random_u64() % 1_000_000;
        }
        nanos
    } else {
        0
    }
}

#[inline]
fn random_u64() -> u64 {
    let mut bytes = [0_u8; 8];
    getrandom::getrandom(&mut bytes).expect("Couldn't get random bytes");
    u64::from_be_bytes(bytes)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashSet};

    #[test]
    fn size_and_alignment() {
        assert_eq!(std::mem::size_of::<Tuid>(), 16);
        assert_eq!(std::mem::align_of::<Tuid>(), 1);
    }

    #[test]
    fn formatting_round_trip() {
        let tuid = Tuid::from_u128(0x182342300c5f8c327a7b4a6e5a379ac4);
        assert_eq!(tuid.to_string(), "182342300C5F8C327a7b4a6e5a379ac4");
        assert_eq!(tuid, tuid.to_string().parse().unwrap());
    }

    #[test]
    fn byte_round_trip() {
        let tuid = Tuid::new();
        assert_eq!(tuid, Tuid::from_bytes(tuid.as_bytes()));
        assert_eq!(tuid, Tuid::from_u128(tuid.as_u128()));
    }

    #[test]
    fn ordering_matches_creation_order() {
        let ids: Vec<Tuid> = (0..10_000).map(|_| Tuid::new()).collect();
        for pair in ids.windows(2) {
            assert!(pair[0] < pair[1], "Tuids must be strictly increasing");
        }
    }

    #[test]
    fn string_ordering_matches_tuid_ordering() {
        let ids: Vec<Tuid> = (0..1_000).map(|_| Tuid::new()).collect();
        let strings: Vec<String> = ids.iter().map(ToString::to_string).collect();
        for pair in strings.windows(2) {
            assert!(pair[0] < pair[1], "String representation must sort identically");
        }
    }

    #[test]
    fn uniqueness() {
        let n = 100_000;
        let ids: Vec<Tuid> = (0..n).map(|_| Tuid::new()).collect();
        assert_eq!(ids.iter().copied().collect::<HashSet<Tuid>>().len(), n);
        assert_eq!(ids.iter().copied().collect::<BTreeSet<Tuid>>().len(), n);
    }

    #[test]
    fn cross_thread_uniqueness() {
        let n_threads = 8;
        let n_per_thread = 10_000;
        let handles: Vec<_> = (0..n_threads)
            .map(|_| {
                std::thread::spawn(move || {
                    (0..n_per_thread).map(|_| Tuid::new()).collect::<Vec<_>>()
                })
            })
            .collect();

        let mut all_ids = HashSet::new();
        for handle in handles {
            for id in handle.join().unwrap() {
                assert!(all_ids.insert(id), "Duplicate Tuid across threads");
            }
        }
        assert_eq!(all_ids.len(), n_threads * n_per_thread);
    }

    #[test]
    fn next_and_incremented_by() {
        let base = Tuid::from_nanos_and_inc(1000, 0);
        assert_eq!(base.next().inc(), 1);
        assert_eq!(base.incremented_by(42).inc(), 42);
        assert_eq!(base.next().nanos_since_epoch(), base.nanos_since_epoch());
    }

    #[test]
    fn constants() {
        assert_eq!(Tuid::ZERO.as_u128(), 0);
        assert_eq!(Tuid::MAX.as_u128(), u128::MAX);
        assert!(Tuid::ZERO < Tuid::MAX);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip() {
        let tuid = Tuid::new();
        let json = serde_json::to_string(&tuid).unwrap();
        let parsed: Tuid = serde_json::from_str(&json).unwrap();
        assert_eq!(tuid, parsed);
    }
}
