//! Global string interning with precomputed hashes.
//!
//! Interned strings are stored as `&'static str` references with a
//! precomputed hash. This gives:
//!
//! - **O(1) equality checks** via hash comparison (no character-by-character)
//! - **O(1) hashing** when used as map keys (hash is precomputed)
//! - **Zero allocation** after the first intern of each unique string
//!
//! Use this for high-frequency, low-cardinality strings like `source`,
//! `topic`, `event_type`, and `entity_type`. Do NOT use for high-cardinality
//! strings like `entity_id` or payload content (they'd never be freed).
//!
//! # Newtype Macro
//!
//! Use [`declare_new_type!`] to create typesafe wrappers:
//!
//! ```
//! chronicle_interner::declare_new_type!(
//!     /// A SaaS data source identifier (e.g., "stripe", "intercom").
//!     pub struct Source;
//! );
//!
//! let s = Source::new("stripe");
//! assert_eq!(s.as_str(), "stripe");
//! ```
//!
//! # Origin
//!
//! Forked from Rerun's `re_string_interner`. Stripped of `re_byte_size`
//! dependency. The `declare_new_type!` macro no longer emits `SizeBytes` impls.

pub mod external {
    pub use nohash_hasher;
    pub use serde;
}

// ---------------------------------------------------------------------------
// Hashing
// ---------------------------------------------------------------------------

/// Deterministic, high-quality hash using fixed seeds.
#[inline]
fn hash(value: impl std::hash::Hash) -> u64 {
    use std::hash::Hasher as _;
    let mut hasher =
        std::hash::BuildHasher::build_hasher(&ahash::RandomState::with_seeds(0, 1, 2, 3));
    value.hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// InternedString
// ---------------------------------------------------------------------------

/// An interned string with a precomputed hash.
///
/// Equality is hash-based (O(1)). Ordering is lexicographic on the
/// underlying `&str`. Interned strings are leaked (`Box::leak`) and
/// never freed -- only use for bounded cardinality values.
#[derive(Copy, Clone, Eq)]
pub struct InternedString {
    hash: u64,
    string: &'static str,
}

impl InternedString {
    /// Intern a string in the global interner.
    #[inline]
    pub fn new(string: &str) -> Self {
        global_intern(string)
    }

    /// The underlying `&'static str` reference.
    #[inline]
    pub fn as_str(&self) -> &'static str {
        self.string
    }

    /// Precomputed hash of the string content.
    #[inline]
    pub fn hash(&self) -> u64 {
        self.hash
    }
}

impl From<&str> for InternedString {
    #[inline]
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for InternedString {
    #[inline]
    fn from(s: String) -> Self {
        Self::new(&s)
    }
}

impl From<&String> for InternedString {
    #[inline]
    fn from(s: &String) -> Self {
        Self::new(s)
    }
}

impl std::cmp::PartialEq for InternedString {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl std::hash::Hash for InternedString {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

impl nohash_hasher::IsEnabled for InternedString {}

impl std::cmp::PartialOrd for InternedString {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::cmp::Ord for InternedString {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.string.cmp(other.string)
    }
}

impl AsRef<str> for InternedString {
    #[inline]
    fn as_ref(&self) -> &str {
        self.string
    }
}

impl std::ops::Deref for InternedString {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Debug for InternedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}

impl std::fmt::Display for InternedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}

impl serde::Serialize for InternedString {
    #[inline]
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_str().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for InternedString {
    #[inline]
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|s| global_intern(&s))
    }
}

// ---------------------------------------------------------------------------
// Internal interner
// ---------------------------------------------------------------------------

#[derive(Default)]
struct StringInterner {
    map: nohash_hasher::IntMap<u64, &'static str>,
}

impl StringInterner {
    fn len(&self) -> usize {
        self.map.len()
    }

    fn intern(&mut self, string: &str) -> InternedString {
        let h = hash(string);
        let static_ref = self
            .map
            .entry(h)
            .or_insert_with(|| Box::leak(Box::<str>::from(string)));
        InternedString {
            hash: h,
            string: static_ref,
        }
    }
}

// ---------------------------------------------------------------------------
// Global singleton
// ---------------------------------------------------------------------------

use parking_lot::Mutex;

static GLOBAL_INTERNER: std::sync::LazyLock<Mutex<StringInterner>> =
    std::sync::LazyLock::new(|| Mutex::new(StringInterner::default()));

fn global_intern(string: &str) -> InternedString {
    GLOBAL_INTERNER.lock().intern(string)
}

/// Total bytes used by the global interner (for monitoring).
pub fn global_bytes_used() -> usize {
    let interner = GLOBAL_INTERNER.lock();
    interner
        .map
        .iter()
        .map(|(k, v): (_, &&str)| std::mem::size_of_val(k) + std::mem::size_of::<&str>() + v.len())
        .sum()
}

// ---------------------------------------------------------------------------
// Newtype macro
// ---------------------------------------------------------------------------

/// Declare a typesafe newtype wrapper around [`InternedString`].
///
/// Generates: `struct`, `new`, `as_str`, `hash`, `From<&str>`, `From<String>`,
/// `Display`, `Debug`, `Deref`, `PartialEq<&str>`, `Hash`, `Eq`, `Ord`,
/// `nohash_hasher::IsEnabled`, and optionally serde support.
///
/// ```
/// chronicle_interner::declare_new_type!(
///     /// An event source like "stripe" or "intercom".
///     pub struct Source;
/// );
/// ```
#[macro_export]
macro_rules! declare_new_type {
    (
        $(#[$meta:meta])*
        $vis:vis struct $Name:ident;
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $Name($crate::InternedString);

        impl $Name {
            #[inline]
            pub fn new(string: &str) -> Self {
                Self($crate::InternedString::new(string))
            }

            #[inline]
            pub fn as_str(&self) -> &'static str {
                self.0.as_str()
            }

            /// Precomputed hash of the underlying string.
            #[inline]
            pub fn hash(&self) -> u64 {
                self.0.hash()
            }
        }

        impl $crate::external::nohash_hasher::IsEnabled for $Name {}

        impl From<&str> for $Name {
            #[inline]
            fn from(s: &str) -> Self { Self::new(s) }
        }

        impl From<String> for $Name {
            #[inline]
            fn from(s: String) -> Self { Self::new(&s) }
        }

        impl AsRef<str> for $Name {
            #[inline]
            fn as_ref(&self) -> &str { self.as_str() }
        }

        impl std::ops::Deref for $Name {
            type Target = str;
            #[inline]
            fn deref(&self) -> &str { self.as_str() }
        }

        impl std::fmt::Debug for $Name {
            #[inline]
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.as_str().fmt(f)
            }
        }

        impl std::fmt::Display for $Name {
            #[inline]
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.as_str().fmt(f)
            }
        }

        impl<'a> PartialEq<&'a str> for $Name {
            #[inline]
            fn eq(&self, other: &&'a str) -> bool { self.as_str() == *other }
        }

        impl<'a> PartialEq<$Name> for &'a str {
            #[inline]
            fn eq(&self, other: &$Name) -> bool { *self == other.as_str() }
        }

        impl $crate::external::serde::Serialize for $Name {
            fn serialize<S: $crate::external::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.as_str().serialize(serializer)
            }
        }

        impl<'de> $crate::external::serde::Deserialize<'de> for $Name {
            fn deserialize<D: $crate::external::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = String::deserialize(deserializer)?;
                Ok(Self::new(&s))
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_deduplication() {
        let mut interner = StringInterner::default();
        assert_eq!(interner.len(), 0);

        let a = interner.intern("hello");
        assert_eq!(interner.len(), 1);

        let b = interner.intern("hello");
        assert_eq!(interner.len(), 1);
        assert_eq!(a, b);

        let c = interner.intern("world");
        assert_eq!(interner.len(), 2);
        assert_ne!(a, c);
    }

    #[test]
    fn equality_is_hash_based() {
        let a = InternedString::new("test");
        let b = InternedString::new("test");
        assert_eq!(a.hash, b.hash);
        assert_eq!(a, b);

        let c = InternedString::new("other");
        assert_ne!(a.hash, c.hash);
        assert_ne!(a, c);
    }

    #[test]
    fn ordering_is_lexicographic() {
        let a = InternedString::new("apple");
        let b = InternedString::new("banana");
        assert!(a < b, "Ordering should be lexicographic on the string");
    }

    #[test]
    fn global_intern_is_consistent() {
        let a = InternedString::new("global_test");
        let b = InternedString::new("global_test");
        assert_eq!(a.as_str() as *const str, b.as_str() as *const str,
            "Same string should return the same &'static str pointer");
    }

    #[test]
    fn concurrent_interning() {
        let handles: Vec<_> = (0..8)
            .map(|i| {
                std::thread::spawn(move || {
                    let s = format!("thread_{i}");
                    let interned = InternedString::new(&s);
                    (s, interned)
                })
            })
            .collect();

        for handle in handles {
            let (original, interned) = handle.join().unwrap();
            assert_eq!(interned.as_str(), original);
        }
    }

    #[test]
    fn newtype_macro_works() {
        declare_new_type!(
            /// A test type.
            pub struct TestType;
        );

        let a = TestType::new("value");
        let b = TestType::new("value");
        assert_eq!(a, b);
        assert_eq!(a.as_str(), "value");
        assert_eq!(format!("{a}"), "value");
        assert_eq!(a, "value");
    }

    #[test]
    fn newtype_partial_eq_str() {
        declare_new_type!(
            /// Test.
            pub struct MyStr;
        );
        let s = MyStr::new("hello");
        assert!(s == "hello");
        assert!("hello" == s);
        assert!(s != "world");
    }

    #[test]
    fn serde_round_trip() {
        let original = InternedString::new("serde_test");
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(json, "\"serde_test\"");
        let parsed: InternedString = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }
}
