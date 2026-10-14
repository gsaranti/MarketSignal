//! The sector and industry reads against their own trailing-year history
//! (`docs/data-sources.md §Report enrichment`): FMP's paid `historical-*-pe` and
//! `historical-*-performance` series reduced to two compact reads per (group, exchange).
//! A read needs [`MIN_SAMPLE`] valid points with the newest no more than
//! [`MAX_STALENESS_DAYS`] old against the ET session date — below either it is
//! [`NotDerivable`], never a numeric default. The wire fetch and the shapers live in `fmp`.

use chrono::{Duration, NaiveDate};

use crate::data_sources::{PeHistory, TrailingReturn};

/// The trailing window each history fetch covers, in calendar days.
pub const WINDOW_DAYS: i64 = 365;

/// The fewest valid points a read rests on (drafted).
pub const MIN_SAMPLE: usize = 60;

/// The oldest the newest valid point may be, in days before the ET session date (drafted).
pub const MAX_STALENESS_DAYS: i64 = 7;

/// A daily `averageChange` beyond this many percentage points either way is inadmissible —
/// a unit-error guard (drafted).
pub const MAX_DAILY_MOVE_PP: f64 = 25.0;

/// One dated value as a shaper read it off the wire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DatedPoint {
    pub date: NaiveDate,
    pub value: f64,
}

/// Why a read could not be derived from a fetched series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotDerivable {
    /// Fewer than [`MIN_SAMPLE`] valid points.
    Sample { valid: usize },
    /// The newest valid point is more than [`MAX_STALENESS_DAYS`] before the session date.
    Stale { newest: NaiveDate },
}

impl NotDerivable {
    /// The one sentence a tracker row carries for the absent read.
    pub fn detail(self) -> String {
        match self {
            NotDerivable::Sample { valid } => {
                format!("{valid} valid points, below the {MIN_SAMPLE}-point floor")
            }
            NotDerivable::Stale { newest } => format!(
                "newest valid point {newest} is more than {MAX_STALENESS_DAYS} days old"
            ),
        }
    }
}

/// The fetch window ending at `to`: `[to − WINDOW_DAYS, to]`.
pub fn window(to: NaiveDate) -> (NaiveDate, NaiveDate) {
    (to - Duration::days(WINDOW_DAYS), to)
}

/// The P/E read: the newest valid print's inclusive percentile among the window's valid
/// prints (ties count as at-or-below), with the window's min / median / max and the P/E at
/// its two ends, so a change in the multiple over the window is a dated comparison rather
/// than an inference from the percentile. A P/E that is non-finite or ≤ 0 is inadmissible
/// against the group's own history.
pub fn pe_history(
    points: &[DatedPoint],
    session_date: NaiveDate,
) -> Result<PeHistory, NotDerivable> {
    let valid = admissible(points, |pe| pe.is_finite() && pe > 0.0);
    let (first, newest) = check_sample(&valid, session_date)?;
    let current = newest.value;
    let at_or_below = valid.iter().filter(|p| p.value <= current).count();
    let mut values: Vec<f64> = valid.iter().map(|p| p.value).collect();
    values.sort_by(f64::total_cmp);
    let n = values.len();
    let median = if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    };
    Ok(PeHistory {
        percentile: 100.0 * at_or_below as f64 / n as f64,
        min: values[0],
        median,
        max: values[n - 1],
        start: first.value,
        latest: current,
        from: first.date.format("%Y-%m-%d").to_string(),
        to: newest.date.format("%Y-%m-%d").to_string(),
    })
}

/// The performance read: the window's daily `averageChange` (percentage points) normalized
/// to daily returns and compounded, `Π(1 + averageChange / 100) − 1`, reported in percent.
/// A non-finite move, or one beyond [`MAX_DAILY_MOVE_PP`] either way, is inadmissible.
pub fn trailing_return(
    points: &[DatedPoint],
    session_date: NaiveDate,
) -> Result<TrailingReturn, NotDerivable> {
    let valid = admissible(points, |change| {
        change.is_finite() && change.abs() <= MAX_DAILY_MOVE_PP
    });
    let (first, newest) = check_sample(&valid, session_date)?;
    let growth: f64 = valid.iter().map(|p| 1.0 + p.value / 100.0).product();
    Ok(TrailingReturn {
        return_pct: (growth - 1.0) * 100.0,
        from: first.date.format("%Y-%m-%d").to_string(),
        to: newest.date.format("%Y-%m-%d").to_string(),
    })
}

/// Sort date-ascending, keep the last row served for each date, then drop the
/// inadmissible values.
fn admissible(points: &[DatedPoint], admit: impl Fn(f64) -> bool) -> Vec<DatedPoint> {
    let mut sorted = points.to_vec();
    // Stable, so rows sharing a date keep their served order and the last one wins below.
    sorted.sort_by_key(|p| p.date);
    let mut deduped: Vec<DatedPoint> = Vec::with_capacity(sorted.len());
    for point in sorted {
        match deduped.last_mut() {
            Some(last) if last.date == point.date => *last = point,
            _ => deduped.push(point),
        }
    }
    deduped.retain(|p| admit(p.value));
    deduped
}

/// The sample floor and the freshness check over the admissible points, returning the
/// window's oldest and newest points.
fn check_sample(
    valid: &[DatedPoint],
    session_date: NaiveDate,
) -> Result<(DatedPoint, DatedPoint), NotDerivable> {
    if valid.len() < MIN_SAMPLE {
        return Err(NotDerivable::Sample { valid: valid.len() });
    }
    let (first, newest) = (valid[0], valid[valid.len() - 1]);
    if (session_date - newest.date).num_days() > MAX_STALENESS_DAYS {
        return Err(NotDerivable::Stale {
            newest: newest.date,
        });
    }
    Ok((first, newest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// `n` consecutive daily points ending at `end`, valued by `f(i)` oldest-first.
    fn series(end: &str, n: usize, f: impl Fn(usize) -> f64) -> Vec<DatedPoint> {
        let end = day(end);
        (0..n)
            .map(|i| DatedPoint {
                date: end - Duration::days((n - 1 - i) as i64),
                value: f(i),
            })
            .collect()
    }

    const SESSION: &str = "2026-10-12";

    #[test]
    fn the_window_is_the_trailing_year_ending_at_the_bound() {
        assert_eq!(window(day("2026-10-12")), (day("2025-10-12"), day("2026-10-12")));
    }

    #[test]
    fn the_percentile_ranks_the_newest_point_inclusively() {
        // Values 1..=60 oldest-first, newest = 60 → every point is at or below it.
        let top = series(SESSION, 60, |i| (i + 1) as f64);
        assert_eq!(pe_history(&top, day(SESSION)).unwrap().percentile, 100.0);
        // Newest = 1 (the minimum) → only itself is at or below.
        let bottom = series(SESSION, 60, |i| (60 - i) as f64);
        let read = pe_history(&bottom, day(SESSION)).unwrap();
        assert!((read.percentile - 100.0 / 60.0).abs() < 1e-9);
    }

    #[test]
    fn ties_count_as_at_or_below() {
        // 30 points at 10, 30 at 20, newest at 10 → the 30 tens are at or below.
        let pts = series(SESSION, 60, |i| if i == 59 || i < 29 { 10.0 } else { 20.0 });
        let read = pe_history(&pts, day(SESSION)).unwrap();
        assert_eq!(read.percentile, 50.0);
    }

    #[test]
    fn the_band_takes_min_median_and_max() {
        // Even count: 1..=60 → median is the mean of 30 and 31.
        let even = pe_history(&series(SESSION, 60, |i| (i + 1) as f64), day(SESSION)).unwrap();
        assert_eq!((even.min, even.median, even.max), (1.0, 30.5, 60.0));
        // Odd count: 1..=61 → median is 31.
        let odd = pe_history(&series(SESSION, 61, |i| (i + 1) as f64), day(SESSION)).unwrap();
        assert_eq!((odd.min, odd.median, odd.max), (1.0, 31.0, 61.0));
        assert_eq!((odd.start, odd.latest), (1.0, 61.0));
        assert_eq!(odd.from, "2026-08-13");
        assert_eq!(odd.to, SESSION);
    }

    #[test]
    fn the_end_prints_carry_the_direction_the_percentile_cannot() {
        // Two windows with the same values in a different order share every rank statistic
        // but run opposite ways: 10 → 20 is an expanding multiple, 30 → 20 a compressing one.
        let expanding = series(SESSION, 60, |i| match i {
            0 => 10.0,
            1 => 30.0,
            _ => 20.0,
        });
        let compressing = series(SESSION, 60, |i| match i {
            0 => 30.0,
            1 => 10.0,
            _ => 20.0,
        });
        let up = pe_history(&expanding, day(SESSION)).unwrap();
        let down = pe_history(&compressing, day(SESSION)).unwrap();
        assert_eq!(
            (up.percentile, up.min, up.median, up.max),
            (down.percentile, down.min, down.median, down.max)
        );
        assert_eq!((up.start, up.latest), (10.0, 20.0));
        assert_eq!((down.start, down.latest), (30.0, 20.0));
    }

    #[test]
    fn a_non_positive_or_non_finite_pe_is_dropped() {
        let mut pts = series(SESSION, 60, |_| 15.0);
        pts[0].value = 0.0;
        assert_eq!(
            pe_history(&pts, day(SESSION)),
            Err(NotDerivable::Sample { valid: 59 })
        );
        pts[0].value = -4.0;
        pts[1].value = f64::NAN;
        assert_eq!(
            pe_history(&pts, day(SESSION)),
            Err(NotDerivable::Sample { valid: 58 })
        );
    }

    #[test]
    fn the_sample_floor_is_sixty_valid_points() {
        assert_eq!(
            pe_history(&series(SESSION, 59, |_| 15.0), day(SESSION)),
            Err(NotDerivable::Sample { valid: 59 })
        );
        assert!(pe_history(&series(SESSION, 60, |_| 15.0), day(SESSION)).is_ok());
        assert_eq!(
            trailing_return(&series(SESSION, 59, |_| 0.1), day(SESSION)),
            Err(NotDerivable::Sample { valid: 59 })
        );
        assert!(trailing_return(&series(SESSION, 60, |_| 0.1), day(SESSION)).is_ok());
    }

    #[test]
    fn the_newest_point_may_be_seven_days_old_not_eight() {
        let seven = series("2026-10-05", 60, |_| 15.0);
        assert!(pe_history(&seven, day(SESSION)).is_ok());
        let eight = series("2026-10-04", 60, |_| 15.0);
        assert_eq!(
            pe_history(&eight, day(SESSION)),
            Err(NotDerivable::Stale {
                newest: day("2026-10-04")
            })
        );
        assert!(trailing_return(&eight, day(SESSION)).is_err());
    }

    #[test]
    fn rows_are_sorted_and_the_last_row_for_a_date_wins() {
        // Served newest-first (the live order), with a duplicate of the newest date whose
        // later row carries the value that must win.
        let mut pts = series(SESSION, 60, |i| (i + 1) as f64);
        pts.reverse();
        pts.push(DatedPoint {
            date: day(SESSION),
            value: 0.5,
        });
        let read = pe_history(&pts, day(SESSION)).unwrap();
        // The newest is now 0.5, below every other point; still 60 distinct dates.
        assert!((read.percentile - 100.0 / 60.0).abs() < 1e-9);
        assert_eq!(read.min, 0.5);
        assert_eq!(read.from, "2026-08-14");
    }

    #[test]
    fn the_return_compounds_percentage_point_moves() {
        // 60 days of +1pp: 1.01^60 − 1.
        let read = trailing_return(&series(SESSION, 60, |_| 1.0), day(SESSION)).unwrap();
        assert!((read.return_pct - (1.01f64.powi(60) - 1.0) * 100.0).abs() < 1e-9);
        // Alternating +10pp / −10pp nets a loss, never zero.
        let chop = trailing_return(
            &series(SESSION, 60, |i| if i % 2 == 0 { 10.0 } else { -10.0 }),
            day(SESSION),
        )
        .unwrap();
        assert!((chop.return_pct - (0.99f64.powi(30) - 1.0) * 100.0).abs() < 1e-9);
        assert_eq!(chop.to, SESSION);
    }

    #[test]
    fn a_move_beyond_twenty_five_points_or_non_finite_is_dropped() {
        let mut pts = series(SESSION, 61, |_| 0.0);
        pts[0].value = 25.0; // at the guard: admissible
        pts[1].value = 25.01;
        pts[2].value = -40.0;
        pts[3].value = f64::INFINITY;
        let read = trailing_return(&pts, day(SESSION));
        assert_eq!(read, Err(NotDerivable::Sample { valid: 58 }));
        let mut pts = series(SESSION, 62, |_| 0.0);
        pts[0].value = 25.0;
        pts[1].value = -25.01;
        let read = trailing_return(&pts, day(SESSION)).unwrap();
        assert!((read.return_pct - 25.0).abs() < 1e-9);
        assert_eq!(read.from, "2026-08-12");
    }

    #[test]
    fn the_detail_names_the_floor_or_the_stale_date() {
        assert_eq!(
            NotDerivable::Sample { valid: 12 }.detail(),
            "12 valid points, below the 60-point floor"
        );
        assert_eq!(
            NotDerivable::Stale {
                newest: day("2026-09-30")
            }
            .detail(),
            "newest valid point 2026-09-30 is more than 7 days old"
        );
    }
}
