//! A tiny, dependency-free UTC clock that formats RFC 3339.
//!
//! Why not `chrono`? The transcript document needs exactly one timestamp format
//! and nothing else date-related. Pulling a full date library (and its
//! transitive tree) into the app's dependency graph for that is not justified,
//! and a hand-rolled implementation is trivially testable against known epochs.
//!
//! The clock is injected via the [`Clock`] trait so tests are deterministic
//! rather than time-dependent.

use std::time::{SystemTime, UNIX_EPOCH};

/// Source of the current time, so tests can pin it.
pub trait Clock: Send + Sync {
    /// Seconds since the Unix epoch.
    fn now_unix_seconds(&self) -> i64;

    /// Current time as an RFC 3339 UTC string, e.g. `2026-10-10T13:00:00Z`.
    fn now_rfc3339(&self) -> String {
        format_rfc3339_utc(self.now_unix_seconds())
    }
}

/// The real system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_seconds(&self) -> i64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            // A clock before 1970 is not a realistic case; clamp rather than
            // panic so transcript creation can never crash the app.
            Err(e) => -(e.duration().as_secs() as i64),
        }
    }
}

/// A clock frozen at a fixed instant. Used by tests.
#[derive(Debug, Clone, Copy)]
pub struct FixedClock {
    seconds: i64,
}

impl FixedClock {
    /// Create a clock pinned to `seconds` since the Unix epoch.
    pub fn new(seconds: i64) -> Self {
        Self { seconds }
    }

    /// A stable instant used across the test-suite: 2026-10-10T13:00:00Z.
    pub fn reference() -> Self {
        Self::new(1_791_637_200)
    }
}

impl Clock for FixedClock {
    fn now_unix_seconds(&self) -> i64 {
        self.seconds
    }
}

/// Days from civil date (year, month, day) — Howard Hinnant's algorithm.
///
/// Valid for all years in the proleptic Gregorian calendar; used in reverse for
/// epoch-seconds to civil conversion.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m + 9) % 12; // Mar=0 .. Feb=11
    let doy = (153 * mp as i64 + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Inverse of [`days_from_civil`]: civil date from days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Format seconds since the Unix epoch as an RFC 3339 UTC timestamp.
///
/// Always emits exactly `YYYY-MM-DDTHH:MM:SSZ`, with no fractional seconds, so
/// exported documents are byte-stable and diffable.
pub fn format_rfc3339_utc(seconds: i64) -> String {
    // Floor division so pre-epoch instants land on the correct day.
    let days = seconds.div_euclid(86_400);
    let secs_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Parse an RFC 3339 UTC timestamp produced by [`format_rfc3339_utc`].
///
/// Accepts the `Z` form and a `+00:00` offset. Deliberately strict: it exists so
/// round-trip tests can assert stability, not to be a general date parser.
pub fn parse_rfc3339_utc(s: &str) -> Option<i64> {
    let s = s.trim();
    let s = s.strip_suffix('Z').or_else(|| s.strip_suffix("+00:00"))?;
    let bytes = s.as_bytes();
    if bytes.len() != 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: u32 = s.get(5..7)?.parse().ok()?;
    let day: u32 = s.get(8..10)?.parse().ok()?;
    let hour: i64 = s.get(11..13)?.parse().ok()?;
    let minute: i64 = s.get(14..16)?.parse().ok()?;
    let second: i64 = s.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_the_unix_epoch() {
        assert_eq!(format_rfc3339_utc(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn formats_a_known_instant() {
        // 2026-10-10T13:00:00Z, independently verified against Python's
        // datetime: int(datetime(2026,10,10,13,tzinfo=UTC).timestamp()).
        assert_eq!(format_rfc3339_utc(1_791_637_200), "2026-10-10T13:00:00Z");
    }

    #[test]
    fn formats_end_of_year_boundary() {
        assert_eq!(format_rfc3339_utc(1_767_225_599), "2025-12-31T23:59:59Z");
        assert_eq!(format_rfc3339_utc(1_767_225_600), "2026-01-01T00:00:00Z");
    }

    #[test]
    fn handles_a_leap_day() {
        // 2024-02-29T12:00:00Z
        assert_eq!(format_rfc3339_utc(1_709_208_000), "2024-02-29T12:00:00Z");
    }

    #[test]
    fn formats_a_pre_epoch_instant_with_floor_division() {
        // Truncating instead of flooring would render this as 1969-12-31T23:59:59Z.
        assert_eq!(format_rfc3339_utc(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn round_trips_across_many_instants_including_negatives() {
        for secs in [
            -1_i64,
            0,
            1,
            86_399,
            86_400,
            1_791_637_200,
            2_000_000_000,
            -2_000_000_000,
        ] {
            let formatted = format_rfc3339_utc(secs);
            assert_eq!(
                parse_rfc3339_utc(&formatted),
                Some(secs),
                "round trip failed for {secs} ({formatted})"
            );
        }
    }

    #[test]
    fn accepts_an_explicit_zero_offset() {
        assert_eq!(
            parse_rfc3339_utc("2026-10-10T13:00:00+00:00"),
            Some(1_791_637_200)
        );
    }

    #[test]
    fn rejects_malformed_timestamps() {
        for bad in [
            "",
            "2026-10-10",
            "2026-10-10 13:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-10-32T00:00:00Z",
            "2026-10-10T25:00:00Z",
            "not-a-date",
            "2026-10-10T13:00:00", // missing zone
        ] {
            assert_eq!(parse_rfc3339_utc(bad), None, "should reject {bad:?}");
        }
    }

    #[test]
    fn fixed_clock_returns_the_pinned_instant() {
        let clock = FixedClock::reference();
        assert_eq!(clock.now_rfc3339(), "2026-10-10T13:00:00Z");
        assert_eq!(clock.now_unix_seconds(), 1_791_637_200);
    }

    #[test]
    fn system_clock_is_monotonic_enough_to_be_after_2020() {
        // Guards against a wildly wrong system clock silently producing
        // nonsense timestamps in exports.
        let now = SystemClock.now_unix_seconds();
        assert!(now > 1_577_836_800, "system clock looks wrong: {now}");
    }
}
