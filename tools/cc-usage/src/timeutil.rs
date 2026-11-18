//! Small, allocation-free time helpers. Parsing ISO-8601 by hand is ~20x faster
//! than going through chrono, which matters when scanning hundreds of MB.

use chrono::{Local, Offset, TimeZone};
use std::collections::HashMap;

/// Days since 1970-01-01 for a proleptic Gregorian date.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "doy-in-month is 1..=31 and the month 1..=12 by construction of the algorithm"
)]
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 0 = Sunday .. 6 = Saturday, for a day number since the epoch.
#[expect(clippy::cast_possible_truncation, reason = "rem_euclid(7) is 0..=6")]
pub fn weekday(day: i64) -> u32 {
    (day + 4).rem_euclid(7) as u32
}

/// Parse `YYYY-MM-DDTHH:MM:SS[.fff](Z|±HH:MM)` into epoch seconds (UTC).
pub fn parse_iso(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let mut n = 0i64;
        for &c in &b[r] {
            if !c.is_ascii_digit() {
                return None;
            }
            n = n * 10 + i64::from(c - b'0');
        }
        Some(n)
    };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "mo is checked to 1..=12 and d to 1..=31 just above"
    )]
    let mut ts = days_from_civil(y, mo as u32, d as u32) * 86_400 + h * 3600 + mi * 60 + se;
    // Skip fractional seconds, then apply an explicit offset if present.
    let mut i = 19;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i + 5 < b.len() && (b[i] == b'+' || b[i] == b'-') {
        let sign = if b[i] == b'+' { 1 } else { -1 };
        let oh = i64::from(b[i + 1] - b'0') * 10 + i64::from(b[i + 2] - b'0');
        let om = i64::from(b[i + 4] - b'0') * 10 + i64::from(b[i + 5] - b'0');
        ts -= sign * (oh * 3600 + om * 60);
    }
    Some(ts)
}

/// Converts UTC timestamps to local calendar days, memoising the UTC offset
/// per 15-minute bucket so a million conversions cost a handful of syscalls.
#[derive(Default)]
pub struct LocalDays {
    memo: HashMap<i64, i64>,
}

impl LocalDays {
    pub fn offset_secs(&mut self, ts: i64) -> i64 {
        *self.memo.entry(ts.div_euclid(900)).or_insert_with(|| {
            Local
                .timestamp_opt(ts, 0)
                .single()
                .map_or(0, |t| i64::from(t.offset().fix().local_minus_utc()))
        })
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "timestamps come from 4-digit-year ISO strings or the clock, so the day number is within about +-3.7 million"
    )]
    pub fn day(&mut self, ts: i64) -> i32 {
        (ts + self.offset_secs(ts)).div_euclid(86_400) as i32
    }
}

pub fn today() -> i32 {
    LocalDays::default().day(chrono::Utc::now().timestamp())
}

/// `MM-DD` for a day number.
pub fn fmt_md(day: i32) -> String {
    let (_, m, d) = civil_from_days(i64::from(day));
    format!("{m:02}-{d:02}")
}

/// `YYYY-MM-DD` for a day number.
pub fn fmt_ymd(day: i32) -> String {
    let (y, m, d) = civil_from_days(i64::from(day));
    format!("{y:04}-{m:02}-{d:02}")
}

pub const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// `HH:MM` in local time.
pub fn fmt_local_hm(ts: i64) -> String {
    match Local.timestamp_opt(ts, 0).single() {
        Some(t) => t.format("%H:%M").to_string(),
        None => String::from("--:--"),
    }
}

/// `MM-DD HH:MM` in local time.
pub fn fmt_local_dt(ts: i64) -> String {
    match Local.timestamp_opt(ts, 0).single() {
        Some(t) => t.format("%m-%d %H:%M").to_string(),
        None => String::from("-"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_roundtrip() {
        for d in [-1000i64, 0, 365, 11_000, 20_000, 20_762] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d);
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(20_762), (2026, 11, 5));
    }

    #[test]
    fn weekday_known() {
        assert_eq!(weekday(0), 4); // 1970-01-01 was a Thursday
        assert_eq!(weekday(days_from_civil(2026, 11, 5)), 4); // Thursday
    }

    #[test]
    fn iso_parsing() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("1970-01-02T00:00:01.999Z"), Some(86_401));
        assert_eq!(parse_iso("1970-01-01T01:00:00+01:00"), Some(0));
        assert_eq!(parse_iso("1970-01-01T00:00:00-05:30"), Some(19_800));
        assert_eq!(parse_iso("garbage"), None);
        assert_eq!(parse_iso("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn parse_iso_rejects_non_numeric_fields() {
        assert!(parse_iso("2026-11-05T10:00:00Z").is_some());
        assert_eq!(parse_iso("2026-1x-05T10:00:00Z"), None);
        assert_eq!(parse_iso("2026-11-05T1o:00:00Z"), None);
        assert_eq!(parse_iso("not a timestamp at all"), None);
        assert_eq!(parse_iso(""), None);
    }

    #[test]
    fn local_time_formats_have_the_documented_shape() {
        let ts = parse_iso("2026-11-05T10:30:00Z").unwrap();
        let hm = fmt_local_hm(ts);
        assert_eq!(hm.len(), 5);
        assert_eq!(&hm[2..3], ":");
        let dt = fmt_local_dt(ts);
        assert_eq!(dt.len(), 11);
        assert_eq!((&dt[2..3], &dt[5..6], &dt[8..9]), ("-", " ", ":"));
        assert!(dt.ends_with(&hm), "the date form ends with the clock form: {dt} / {hm}");
    }
}
