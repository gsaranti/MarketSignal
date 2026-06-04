//! Scheduled-window math for the Weekly Market Report job.
//!
//! The job fires Sunday 9:00 AM local time (`docs/scheduling.md §Weekly Market
//! Report Job`). This module turns that rule into two pure functions over a
//! caller-supplied `now` — the live timer uses them to compute how long to
//! sleep, and missed-job detection uses them to find the window that should have
//! fired. They are generic over the timezone and take `now` as a parameter (no
//! `Local::now()` inside), so every window is deterministic under test with a
//! `FixedOffset`; production passes `chrono::Local`.

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, TimeZone};

/// The scheduled hour, local time: 09:00.
const SCHEDULED_HOUR: u32 = 9;

/// 09:00 local on `date`, in `tz`. 09:00 sits well clear of the usual pre-dawn
/// DST transition, so the ambiguous/nonexistent cases are defensive: an
/// ambiguous hour takes the earliest instant, and a (practically impossible)
/// gap at 09:00 steps one hour on so the window still resolves to an instant.
fn at_scheduled_hour<Tz: TimeZone>(date: NaiveDate, tz: &Tz) -> DateTime<Tz> {
    let naive = date
        .and_hms_opt(SCHEDULED_HOUR, 0, 0)
        .expect("09:00:00 is a valid wall-clock time");
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(earliest, _) => earliest,
        LocalResult::None => tz
            .from_local_datetime(&(naive + Duration::hours(1)))
            .earliest()
            .expect("a valid instant near 09:00 local"),
    }
}

/// The most recent Sunday 09:00 (in `now`'s timezone) at or before `now`. On a
/// Sunday before 09:00 the most recent window is the previous week's, not
/// today's still-future one.
pub fn previous_window_at_or_before<Tz: TimeZone>(now: DateTime<Tz>) -> DateTime<Tz> {
    let tz = now.timezone();
    let days_since_sunday = now.weekday().num_days_from_sunday() as i64;
    let this_or_last_sunday = now.date_naive() - Duration::days(days_since_sunday);
    let candidate = at_scheduled_hour(this_or_last_sunday, &tz);
    if candidate > now {
        // Sunday, but before 09:00 — the most recent window was a week ago.
        at_scheduled_hour(this_or_last_sunday - Duration::days(7), &tz)
    } else {
        candidate
    }
}

/// The next Sunday 09:00 (in `now`'s timezone) strictly after `now`. Exactly one
/// week after the most recent window, so a `now` landing precisely on a window
/// yields the following week's, never the same instant.
pub fn next_run_after<Tz: TimeZone>(now: DateTime<Tz>) -> DateTime<Tz> {
    let prev = previous_window_at_or_before(now.clone());
    at_scheduled_hour(prev.date_naive() + Duration::days(7), &now.timezone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    /// A non-UTC zone (+02:00) so the tests also pin that windows are computed in
    /// the supplied timezone, not silently in UTC.
    fn tz() -> FixedOffset {
        FixedOffset::east_opt(2 * 3600).unwrap()
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<FixedOffset> {
        tz().with_ymd_and_hms(y, m, d, h, min, 0).single().unwrap()
    }

    // 2026-06-07 is a Sunday; 2026-06-03 (the project's "today") is a Wednesday.

    #[test]
    fn midweek_previous_is_the_prior_sunday_and_next_is_the_coming_sunday() {
        let now = at(2026, 6, 3, 12, 0); // Wednesday noon
        assert_eq!(previous_window_at_or_before(now), at(2026, 5, 31, 9, 0));
        assert_eq!(next_run_after(now), at(2026, 6, 7, 9, 0));
    }

    #[test]
    fn sunday_before_nine_has_not_fired_this_week() {
        let now = at(2026, 6, 7, 8, 59); // Sunday 08:59
        assert_eq!(previous_window_at_or_before(now), at(2026, 5, 31, 9, 0));
        assert_eq!(next_run_after(now), at(2026, 6, 7, 9, 0));
    }

    #[test]
    fn sunday_exactly_at_nine_is_the_current_window() {
        let now = at(2026, 6, 7, 9, 0); // Sunday 09:00 sharp
        assert_eq!(previous_window_at_or_before(now), at(2026, 6, 7, 9, 0));
        // Strictly-after: the boundary instant yields the following week.
        assert_eq!(next_run_after(now), at(2026, 6, 14, 9, 0));
    }

    #[test]
    fn sunday_after_nine_has_already_fired_this_week() {
        let now = at(2026, 6, 7, 9, 1); // Sunday 09:01
        assert_eq!(previous_window_at_or_before(now), at(2026, 6, 7, 9, 0));
        assert_eq!(next_run_after(now), at(2026, 6, 14, 9, 0));
    }
}
