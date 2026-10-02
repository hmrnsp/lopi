//! UTC time formatting without a date-time crate. Times before 1970 are clamped to the
//! epoch (they never occur in practice and are not worth an error path).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Utc {
    year: i64,
    month: u32,
    day: u32,
    hour: u64,
    minute: u64,
    second: u64,
    millis: u32,
}

fn utc(time: SystemTime) -> Utc {
    let since_epoch = time.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    let secs = since_epoch.as_secs();
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    Utc {
        year,
        month,
        day,
        hour: rem / 3600,
        minute: rem % 3600 / 60,
        second: rem % 60,
        millis: since_epoch.subsec_millis(),
    }
}

/// `2026-10-01T10:15:30Z`, used for `updated_at` and the history file.
pub fn rfc3339_utc(time: SystemTime) -> String {
    let t = utc(time);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

pub fn now_rfc3339() -> String {
    rfc3339_utc(SystemTime::now())
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

/// Short relative time for tables: `just now`, `5m ago`, `3h ago`, `12d ago`.
/// A time in the future (clock changed) reads as `just now`.
pub fn ago(now: u64, then: u64) -> String {
    let secs = now.saturating_sub(then);
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86_400 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

/// `20261001T101530123Z`: sortable, millisecond precision, valid in file names everywhere
/// (no `:`), used for backup file names.
pub fn compact_utc(time: SystemTime) -> String {
    let t = utc(time);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}{:03}Z",
        t.year, t.month, t.day, t.hour, t.minute, t.second, t.millis
    )
}

/// Reads a [`compact_utc`] stamp back. `None` for anything else, including dates before
/// 1970 and out-of-range fields.
pub fn parse_compact(stamp: &str) -> Option<SystemTime> {
    let digits = stamp.strip_suffix('Z')?;
    let (date, time) = digits.split_once('T')?;
    if date.len() != 8
        || time.len() != 9
        || !(date.bytes().chain(time.bytes())).all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let num = |s: &str| s.parse::<u32>().ok();
    let (year, month, day) = (num(&date[..4])?, num(&date[4..6])?, num(&date[6..])?);
    let (hour, minute, second) = (num(&time[..2])?, num(&time[2..4])?, num(&time[4..6])?);
    let millis = num(&time[6..])?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(i64::from(year), month, day);
    let secs = days * 86_400 + i64::from(hour * 3600 + minute * 60 + second);
    let secs = u64::try_from(secs).ok()?;
    Some(UNIX_EPOCH + Duration::from_secs(secs) + Duration::from_millis(u64::from(millis)))
}

/// (year, month, day) → days since 1970-01-01. Howard Hinnant's `days_from_civil`.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = i64::from((month + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Days since 1970-01-01 → (year, month, day) in the proleptic Gregorian calendar.
/// Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn compact_round_trips() {
        for secs in [0, 951_782_400, 1_700_000_000, 4_102_444_799] {
            let time = at(secs) + Duration::from_millis(123);
            assert_eq!(parse_compact(&compact_utc(time)), Some(time), "{secs}");
        }
        for bad in [
            "",
            "20261001T101530123",
            "2026-10-01T10:15:30Z",
            "20261301T101530123Z",
            "20261001T246030123Z",
            "x0261001T101530123Z",
        ] {
            assert_eq!(parse_compact(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn rfc3339() {
        assert_eq!(rfc3339_utc(at(0)), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(at(951_782_400)), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339_utc(at(1_700_000_000)), "2023-11-14T22:13:20Z");
        assert_eq!(rfc3339_utc(at(4_102_444_799)), "2099-12-31T23:59:59Z");
        assert_eq!(rfc3339_utc(at(4_102_444_800)), "2100-01-01T00:00:00Z");
        assert_eq!(
            rfc3339_utc(UNIX_EPOCH - Duration::from_secs(5)),
            "1970-01-01T00:00:00Z"
        );
    }

    #[test]
    fn relative() {
        let cases = [
            (0, "just now"),
            (59, "just now"),
            (60, "1m ago"),
            (3599, "59m ago"),
            (3600, "1h ago"),
            (86_399, "23h ago"),
            (86_400, "1d ago"),
            (40 * 86_400, "40d ago"),
        ];
        for (elapsed, expected) in cases {
            assert_eq!(ago(1_000_000 + elapsed, 1_000_000), expected, "{elapsed}");
        }
        assert_eq!(ago(10, 20), "just now");
    }

    #[test]
    fn compact() {
        let time = at(1_700_000_000) + Duration::from_millis(7);
        assert_eq!(compact_utc(time), "20231114T221320007Z");
    }
}
