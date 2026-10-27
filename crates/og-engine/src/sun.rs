//! Where the sun is in the sky at a time and a place, after the low-precision formulas of
//! the Astronomical Almanac, as the U.S. Naval Observatory gives them. Good to about a
//! hundredth of a degree for this century, which is plenty for shadows.

/// The sun's azimuth, clockwise from north, and its altitude above the horizon, in degrees,
/// at `unix_seconds` (UTC), seen from latitude and longitude in degrees.
pub fn position(unix_seconds: f64, lat: f64, lon: f64) -> (f64, f64) {
    // Days since noon on 1 January 2000, UTC.
    let n = unix_seconds / 86_400.0 - 10_957.5;
    let rad = std::f64::consts::PI / 180.0;
    let l = (280.460 + 0.985_647_4 * n).rem_euclid(360.0);
    let g = (357.528 + 0.985_600_3 * n).rem_euclid(360.0) * rad;
    let lambda = (l + 1.915 * g.sin() + 0.020 * (2.0 * g).sin()) * rad;
    let eps = (23.439 - 0.000_000_4 * n) * rad;
    let ra = (eps.cos() * lambda.sin()).atan2(lambda.cos());
    let dec = (eps.sin() * lambda.sin()).asin();
    // The hour angle, from the sidereal time at Greenwich and the longitude.
    let gmst = (18.697_374_558 + 24.065_709_824_419_08 * n).rem_euclid(24.0);
    let h = (gmst * 15.0 + lon) * rad - ra;
    let phi = lat * rad;
    let alt = (phi.sin() * dec.sin() + phi.cos() * dec.cos() * h.cos()).asin();
    let az = h.sin().atan2(h.cos() * phi.sin() - dec.tan() * phi.cos()) / rad + 180.0;
    (az.rem_euclid(360.0), alt / rad)
}

/// Which clock a time is read on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Clock {
    /// Swedish time, as in the rest of central Europe: an hour after UTC, two in summer.
    Swedish,
    Utc,
    /// The clock of this computer.
    Computer,
}

/// The last Sunday of a month, at 01:00 UTC, when summer time begins or ends in the EU.
fn last_sunday_1_utc(year: i32, month: u32) -> Option<chrono::NaiveDateTime> {
    use chrono::Datelike;
    let next = if month == 12 { chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1)? } else { chrono::NaiveDate::from_ymd_opt(year, month + 1, 1)? };
    let last = next.pred_opt()?;
    let back = last.weekday().num_days_from_sunday() as i64;
    (last - chrono::Duration::days(back)).and_hms_opt(1, 0, 0)
}

/// The hours Swedish time is ahead of UTC at a moment in UTC: 2 in summer time, else 1.
pub fn swedish_offset(utc: chrono::NaiveDateTime) -> i64 {
    use chrono::Datelike;
    match (last_sunday_1_utc(utc.year(), 3), last_sunday_1_utc(utc.year(), 10)) {
        (Some(start), Some(end)) if utc >= start && utc < end => 2,
        _ => 1,
    }
}

/// The time, in seconds since 1970 UTC, of a date and a time on a clock.
pub fn time_on(date: &str, time: &str, clock: Clock) -> Option<f64> {
    use chrono::{Datelike, TimeZone};
    let d = chrono::NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").ok()?;
    let t = chrono::NaiveTime::parse_from_str(time.trim(), "%H:%M").or_else(|_| chrono::NaiveTime::parse_from_str(time.trim(), "%H:%M:%S")).ok()?;
    let local = d.and_time(t);
    match clock {
        Clock::Utc => Some(local.and_utc().timestamp() as f64),
        Clock::Computer => Some(chrono::Local.from_local_datetime(&local).earliest()?.timestamp() as f64),
        Clock::Swedish => {
            // Summer time from the last Sunday in March to the last Sunday in October.
            let (start, end) = (last_sunday_1_utc(d.year(), 3)?, last_sunday_1_utc(d.year(), 10)?);
            let as_winter = local - chrono::Duration::hours(1);
            let summer = as_winter >= start && local - chrono::Duration::hours(2) < end;
            let utc = if summer { local - chrono::Duration::hours(2) } else { as_winter };
            Some(utc.and_utc().timestamp() as f64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_stands_where_it_should() {
        // Göteborg at midsummer, at about solar noon (11:12 UTC): due south, 55.7° up.
        let t = 1_782_040_320.0; // 2026-06-21 11:12 UTC
        let (az, alt) = position(t, 57.7, 12.0);
        assert!((az - 180.0).abs() < 1.5, "{}", az);
        assert!((alt - 55.7).abs() < 0.3, "{}", alt);
        // At the equinox in the evening the sun is low in the west.
        let (az, alt) = position(1_773_946_800.0, 57.7, 12.0); // 2026-03-19 19:00 UTC
        assert!(alt < 0.0 && az > 250.0 && az < 300.0, "{} {}", az, alt);
        // Swedish time: two hours after UTC in summer, one in winter.
        let utc = |d: &str, t: &str| time_on(d, t, Clock::Utc).unwrap();
        assert_eq!(time_on("2026-06-21", "13:12", Clock::Swedish).unwrap(), utc("2026-06-21", "11:12"));
        assert_eq!(time_on("2026-12-24", "15:00", Clock::Swedish).unwrap(), utc("2026-12-24", "14:00"));
        // Summer time begins at 02:00 on 29 March 2026 and ends at 03:00 on 25 October.
        assert_eq!(time_on("2026-03-29", "01:30", Clock::Swedish).unwrap(), utc("2026-03-29", "00:30"));
        assert_eq!(time_on("2026-03-29", "03:30", Clock::Swedish).unwrap(), utc("2026-03-29", "01:30"));
        assert_eq!(time_on("2026-10-25", "04:00", Clock::Swedish).unwrap(), utc("2026-10-25", "03:00"));
    }
}
