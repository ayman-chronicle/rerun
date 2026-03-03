//! Time range type for temporal queries.
//!
//! [`TimeRange`] represents a closed interval `[min, max]` of timestamps.
//! It provides factory methods for common patterns like "last N days".

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// A closed time interval `[min, max]`.
///
/// Both endpoints are inclusive. Use the factory methods for common
/// patterns, or construct directly for custom ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    min: DateTime<Utc>,
    max: DateTime<Utc>,
}

impl TimeRange {
    /// Create a range from explicit min and max timestamps.
    ///
    /// Returns `None` if `min > max`.
    pub fn new(min: DateTime<Utc>, max: DateTime<Utc>) -> Option<Self> {
        if min > max {
            return None;
        }
        Some(Self { min, max })
    }

    /// Range covering the last `n` days from now.
    pub fn last_days(n: i64) -> Self {
        let now = Utc::now();
        Self {
            min: now - Duration::days(n),
            max: now,
        }
    }

    /// Range covering the last `n` hours from now.
    pub fn last_hours(n: i64) -> Self {
        let now = Utc::now();
        Self {
            min: now - Duration::hours(n),
            max: now,
        }
    }

    /// Range from a specific start time until now.
    pub fn since(start: DateTime<Utc>) -> Self {
        Self {
            min: start,
            max: Utc::now(),
        }
    }

    /// The start of the range (inclusive).
    #[inline]
    pub fn min(&self) -> DateTime<Utc> {
        self.min
    }

    /// The end of the range (inclusive).
    #[inline]
    pub fn max(&self) -> DateTime<Utc> {
        self.max
    }

    /// Duration of the range.
    #[inline]
    pub fn duration(&self) -> Duration {
        self.max - self.min
    }

    /// Whether a timestamp falls within this range (inclusive).
    #[inline]
    pub fn contains(&self, t: DateTime<Utc>) -> bool {
        self.min <= t && t <= self.max
    }

    /// Whether two ranges overlap.
    pub fn overlaps(&self, other: &Self) -> bool {
        self.min <= other.max && other.min <= self.max
    }
}

impl std::fmt::Display for TimeRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{}", self.min.format("%Y-%m-%d"), self.max.format("%Y-%m-%d"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).unwrap()
    }

    #[test]
    fn valid_range() {
        let range = TimeRange::new(dt(2024, 1, 1), dt(2024, 3, 31)).unwrap();
        assert_eq!(range.min(), dt(2024, 1, 1));
        assert_eq!(range.max(), dt(2024, 3, 31));
    }

    #[test]
    fn invalid_range_returns_none() {
        assert!(TimeRange::new(dt(2024, 12, 31), dt(2024, 1, 1)).is_none());
    }

    #[test]
    fn same_point_is_valid() {
        let t = dt(2024, 6, 15);
        let range = TimeRange::new(t, t).unwrap();
        assert!(range.contains(t));
        assert_eq!(range.duration(), Duration::zero());
    }

    #[test]
    fn contains() {
        let range = TimeRange::new(dt(2024, 1, 1), dt(2024, 12, 31)).unwrap();
        assert!(range.contains(dt(2024, 6, 15)));
        assert!(range.contains(dt(2024, 1, 1)));
        assert!(range.contains(dt(2024, 12, 31)));
        assert!(!range.contains(dt(2023, 12, 31)));
        assert!(!range.contains(dt(2025, 1, 1)));
    }

    #[test]
    fn overlaps() {
        let a = TimeRange::new(dt(2024, 1, 1), dt(2024, 6, 30)).unwrap();
        let b = TimeRange::new(dt(2024, 3, 1), dt(2024, 12, 31)).unwrap();
        let c = TimeRange::new(dt(2024, 7, 1), dt(2024, 12, 31)).unwrap();

        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));
        assert!(!a.overlaps(&c));
    }

    #[test]
    fn last_days_is_reasonable() {
        let range = TimeRange::last_days(30);
        let duration = range.duration();
        assert!(duration.num_days() >= 29 && duration.num_days() <= 30);
    }

    #[test]
    fn display() {
        let range = TimeRange::new(dt(2024, 1, 15), dt(2024, 3, 20)).unwrap();
        assert_eq!(range.to_string(), "2024-01-15..2024-03-20");
    }
}
