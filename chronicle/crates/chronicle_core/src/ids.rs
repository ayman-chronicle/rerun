//! Newtype ID wrappers for type safety.
//!
//! Every identifier in Chronicle has its own type so the compiler
//! catches mix-ups. You cannot accidentally pass an `OrgId` where
//! an `EntityId` is expected.
//!
//! There are two families of IDs:
//!
//! - **Tuid-based** (`EventId`, `LinkId`): Time-ordered, generated internally.
//!   These use [`chronicle_tuid::Tuid`] and sort in creation order.
//!
//! - **Interned string-based** (`OrgId`, `Source`, `Topic`, `EventType`,
//!   `EntityType`): Low-cardinality strings that appear in every event.
//!   Interned for O(1) comparison.
//!
//! - **Plain string** (`EntityId`): High-cardinality external identifiers
//!   (e.g., "cust_123"). Not interned because there are too many unique values.

use chronicle_tuid::Tuid;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Tuid-based IDs (time-ordered, internally generated)
// ---------------------------------------------------------------------------

/// Generates a newtype around [`Tuid`] with Display (prefixed), FromStr,
/// Serialize, Deserialize, and all comparison traits.
macro_rules! declare_tuid_id {
    (
        $(#[$meta:meta])*
        $Name:ident, $prefix:literal
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $Name(Tuid);

        impl $Name {
            /// Generate a new unique ID based on the current time.
            #[inline]
            pub fn new() -> Self {
                Self(Tuid::new())
            }

            /// Wrap an existing [`Tuid`].
            #[inline]
            pub fn from_tuid(tuid: Tuid) -> Self {
                Self(tuid)
            }

            /// The underlying [`Tuid`].
            #[inline]
            pub fn as_tuid(&self) -> Tuid {
                self.0
            }
        }

        impl Default for $Name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $Name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}_{}", $prefix, self.0)
            }
        }

        impl std::str::FromStr for $Name {
            type Err = IdParseError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let hex = s
                    .strip_prefix(concat!($prefix, "_"))
                    .unwrap_or(s);
                let tuid = hex.parse::<Tuid>()
                    .map_err(|_| IdParseError::InvalidHex(s.to_string()))?;
                Ok(Self(tuid))
            }
        }

        impl Serialize for $Name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.to_string().serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $Name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = String::deserialize(deserializer)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

declare_tuid_id!(
    /// Unique identifier for an event. Time-ordered.
    EventId, "evt"
);

declare_tuid_id!(
    /// Unique identifier for an event link. Time-ordered.
    LinkId, "lnk"
);

// ---------------------------------------------------------------------------
// Interned string IDs (low-cardinality, O(1) comparison)
// ---------------------------------------------------------------------------

chronicle_interner::declare_new_type!(
    /// Tenant organization identifier (e.g., "org_acme").
    pub struct OrgId;
);

chronicle_interner::declare_new_type!(
    /// Event source (e.g., "stripe", "intercom", "product").
    pub struct Source;
);

chronicle_interner::declare_new_type!(
    /// Topic within a source (e.g., "payments", "tickets").
    pub struct Topic;
);

chronicle_interner::declare_new_type!(
    /// Event type (e.g., "payment_intent.succeeded").
    pub struct EventType;
);

chronicle_interner::declare_new_type!(
    /// Entity type (e.g., "customer", "account", "ticket").
    pub struct EntityType;
);

// ---------------------------------------------------------------------------
// Plain string IDs (high-cardinality)
// ---------------------------------------------------------------------------

/// An entity identifier like "cust_123" or "acc_456".
///
/// Not interned because entity IDs have unbounded cardinality.
/// Stored as a plain `String`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId(pub String);

impl EntityId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<&str> for EntityId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for EntityId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

// ---------------------------------------------------------------------------
// Confidence (validated newtype)
// ---------------------------------------------------------------------------

/// A confidence score between 0.0 and 1.0 (inclusive).
///
/// Construction validates the range at creation time, so downstream
/// code can trust the value without re-checking.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Confidence(f32);

impl Confidence {
    /// Create a new confidence score. Returns an error if the value
    /// is outside `[0.0, 1.0]`.
    pub fn new(value: f32) -> Result<Self, ConfidenceError> {
        if !(0.0..=1.0).contains(&value) {
            return Err(ConfidenceError::OutOfRange(value));
        }
        Ok(Self(value))
    }

    /// The raw f32 value, guaranteed to be in `[0.0, 1.0]`.
    #[inline]
    pub fn value(&self) -> f32 {
        self.0
    }
}

impl std::fmt::Display for Confidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.2}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Error parsing an ID from a string.
#[derive(Debug, Clone, thiserror::Error)]
pub enum IdParseError {
    #[error("invalid hex in ID: {0:?}")]
    InvalidHex(String),
}

/// Error constructing a [`Confidence`] value.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfidenceError {
    #[error("confidence {0} is outside valid range [0.0, 1.0]")]
    OutOfRange(f32),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_id_display_and_parse() {
        let id = EventId::new();
        let s = id.to_string();
        assert!(s.starts_with("evt_"), "EventId should have evt_ prefix: {s}");
        let parsed: EventId = s.parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn event_id_parse_without_prefix() {
        let id = EventId::new();
        let hex = id.as_tuid().to_string();
        let parsed: EventId = hex.parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn link_id_display_and_parse() {
        let id = LinkId::new();
        let s = id.to_string();
        assert!(s.starts_with("lnk_"));
        let parsed: LinkId = s.parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn tuid_ids_are_time_ordered() {
        let ids: Vec<EventId> = (0..100).map(|_| EventId::new()).collect();
        for pair in ids.windows(2) {
            assert!(pair[0] < pair[1]);
        }
    }

    #[test]
    fn interned_ids_equality() {
        let a = Source::new("stripe");
        let b = Source::new("stripe");
        assert_eq!(a, b);
        assert_eq!(a, "stripe");
    }

    #[test]
    fn interned_ids_different_types_dont_mix() {
        let source = Source::new("test");
        let topic = Topic::new("test");
        // These are different types -- they can't be compared at compile time.
        // This test just verifies they both exist and work independently.
        assert_eq!(source.as_str(), topic.as_str());
    }

    #[test]
    fn entity_id_from_str() {
        let id = EntityId::new("cust_123");
        assert_eq!(id.as_str(), "cust_123");
        assert_eq!(id, EntityId::from("cust_123"));
    }

    #[test]
    fn confidence_valid_range() {
        assert!(Confidence::new(0.0).is_ok());
        assert!(Confidence::new(0.5).is_ok());
        assert!(Confidence::new(1.0).is_ok());
    }

    #[test]
    fn confidence_rejects_invalid() {
        assert!(Confidence::new(-0.1).is_err());
        assert!(Confidence::new(1.1).is_err());
        assert!(Confidence::new(f32::NAN).is_err());
    }

    #[test]
    fn confidence_display() {
        let c = Confidence::new(0.85).unwrap();
        assert_eq!(c.to_string(), "0.85");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn event_id_serde_round_trip() {
        let id = EventId::new();
        let json = serde_json::to_string(&id).unwrap();
        let parsed: EventId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, parsed);
    }
}
