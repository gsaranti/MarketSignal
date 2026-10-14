//! Market session state and ET session dates: where the US equity market
//! stands relative to its regular hours (9:30 AM–4:00 PM America/New_York,
//! Mon–Fri) at a run's `as_of`, so the main agent narrates the day in the
//! right tense. The Eastern offset is DST-correct; market holidays and
//! half-days are deliberately out of scope (a holiday classifies as a normal
//! weekday) rather than carrying a table that needs yearly maintenance.

use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc, Weekday};
use chrono_tz::America::New_York;
use chrono_tz::Tz;

/// The ET calendar date of a UTC instant, the market's session date. An
/// evening-ET run has already rolled to the next UTC date, so a UTC date
/// prefix would date it one session late. Every session-keyed date reads
/// through here (`docs/portfolio-analysis.md` §Outcome learning, §The quick
/// check).
pub fn et_session_date(utc: DateTime<Utc>) -> NaiveDate {
    utc.with_timezone(&New_York).date_naive()
}

/// `YYYY-MM-DD HH:MM ET`, the label each baseline-history point carries; the
/// time keeps same-session regenerations distinct.
pub fn et_stamp(utc: DateTime<Utc>) -> String {
    utc.with_timezone(&New_York)
        .format("%Y-%m-%d %H:%M ET")
        .to_string()
}

/// The ET session date of a persisted timestamp string. A string that does
/// not parse as an RFC3339 instant falls back to its bare `YYYY-MM-DD` prefix,
/// so a date-only value is taken as-is and a malformed timestamp degrades
/// rather than vanishing; `None` when neither parses.
pub fn et_date_of(stamp: &str) -> Option<NaiveDate> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(stamp) {
        return Some(dt.with_timezone(&New_York).date_naive());
    }
    stamp
        .get(..10)
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
}

/// `date` plus `months` calendar months, a day past the target month's end
/// clamping to its last day (`docs/portfolio-analysis.md` §Outcome learning).
/// The frontend `src/etDate.ts` is behaviorally equivalent on the pinned
/// [`tests::CALENDAR_MONTH_CASES`], not a port: change one side and re-pin
/// both case tables.
pub fn add_calendar_months(date: NaiveDate, months: u32) -> Option<NaiveDate> {
    date.checked_add_months(chrono::Months::new(months))
}

/// Minutes since ET midnight.
const SESSION_OPEN_MINUTES: u32 = 9 * 60 + 30;
/// Minutes since ET midnight.
const SESSION_CLOSE_MINUTES: u32 = 16 * 60;
const SESSION_HOURS: f64 = 6.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketSession {
    /// A weekday before the 9:30 AM ET open.
    PreOpen,
    /// Within regular trading hours (9:30 AM–4:00 PM ET).
    Open,
    /// A weekday after the 4:00 PM ET close.
    AfterClose,
    Weekend,
}

#[derive(Debug, Clone)]
struct Snapshot {
    /// `as_of` in US/Eastern.
    et: DateTime<Tz>,
    session: MarketSession,
    /// Minutes since ET midnight.
    minutes: u32,
}

/// `Default` is the no-context state (the offline/stub path has no real
/// `as_of`): [`Self::main_agent_guidance`] yields `None` and the prompt omits
/// the block.
#[derive(Debug, Clone, Default)]
pub struct MarketClock {
    snapshot: Option<Snapshot>,
}

impl MarketClock {
    pub fn from_utc(as_of: DateTime<Utc>) -> Self {
        let et = as_of.with_timezone(&New_York);
        let minutes = et.hour() * 60 + et.minute();
        let session = match et.weekday() {
            Weekday::Sat | Weekday::Sun => MarketSession::Weekend,
            _ if minutes < SESSION_OPEN_MINUTES => MarketSession::PreOpen,
            // Half-open [open, close): 9:30 is open, 16:00 is closed.
            _ if minutes < SESSION_CLOSE_MINUTES => MarketSession::Open,
            _ => MarketSession::AfterClose,
        };
        Self {
            snapshot: Some(Snapshot {
                et,
                session,
                minutes,
            }),
        }
    }

    /// `None` on the no-context path.
    pub fn session(&self) -> Option<MarketSession> {
        self.snapshot.as_ref().map(|s| s.session)
    }

    /// The report date as `YYYY-MM-DD` in ET (the market day), so a late-evening
    /// run still anchors to the correct trading date; the recency anchor for
    /// the news-selection stages. `None` on the no-context path.
    pub fn report_date(&self) -> Option<String> {
        self.snapshot
            .as_ref()
            .map(|s| s.et.format("%Y-%m-%d").to_string())
    }

    /// The session block appended to the main agent's prompt. Always begins
    /// with the `Market session:` marker so the wiring is assertable; `None` on
    /// the no-context path, so the prompt omits the block.
    pub fn main_agent_guidance(&self) -> Option<String> {
        let snap = self.snapshot.as_ref()?;
        let stamp = snap.et.format("%A, %B %-d, %Y, %-I:%M %p ET");
        Some(match snap.session {
            MarketSession::Open => {
                let into = f64::from(snap.minutes - SESSION_OPEN_MINUTES) / 60.0;
                let remaining = f64::from(SESSION_CLOSE_MINUTES - snap.minutes) / 60.0;
                format!(
                    "Market session: as of {stamp}, the US equity market is OPEN — about \
                     {into:.1} hours into the regular {SESSION_HOURS}-hour session (9:30 AM–4:00 \
                     PM ET), with ~{remaining:.1} hours still to trade. The index, sector, and \
                     mover figures in the baseline are LIVE INTRADAY levels, and each day-percent \
                     move is the change so far today versus the prior session's close — not a \
                     completed-session result. Write the day in the present tense: the move is \
                     provisional and can still reverse into the close, so do not describe the \
                     session as finished ('closed', 'ended the day', 'on the session'). Separate \
                     what has already happened intraday from what is still unresolved into the \
                     close."
                )
            }
            MarketSession::PreOpen => format!(
                "Market session: as of {stamp}, the US equity market has NOT YET OPENED today — \
                 the regular session begins at 9:30 AM ET. The baseline's index and sector figures \
                 reflect the prior session's close; any day move shown is a pre-market indication, \
                 not today's trading. Frame today as still ahead — what the session opens into, and \
                 the levels or events that will set its tone — rather than narrating a day that has \
                 not traded."
            ),
            MarketSession::AfterClose => format!(
                "Market session: as of {stamp}, the US equity market has CLOSED for the day (the \
                 regular session ended at 4:00 PM ET). The baseline's day-percent moves are the \
                 completed session's result, so past-tense narration of the day is correct."
            ),
            MarketSession::Weekend => format!(
                "Market session: as of {stamp}, the US equity market is CLOSED for the weekend. \
                 The baseline's figures reflect the last session's close; narrate that close in the \
                 past tense and look ahead to the next session's open."
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// June 2026 is EDT (UTC-4); January 2026 is EST (UTC-5).
    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn open_midsession_summer_edt() {
        // 2026-06-23 is a Tuesday. 16:04 UTC = 12:04 EDT, ~2.5h into the session.
        let clock = MarketClock::from_utc(utc(2026, 6, 23, 16, 4));
        assert_eq!(clock.session(), Some(MarketSession::Open));
        let g = clock.main_agent_guidance().unwrap();
        assert!(g.starts_with("Market session:"), "carries the marker: {g}");
        assert!(g.contains("OPEN"));
        assert!(g.contains("INTRADAY"), "names the live/intraday framing: {g}");
        assert!(g.contains("hours into the regular"), "captions progress: {g}");
        assert!(g.contains("12:04 PM ET"), "stamps the ET wall-clock time: {g}");
    }

    #[test]
    fn pre_open_weekday() {
        // 2026-06-23 (Tue) 13:00 UTC = 09:00 EDT → before the 9:30 open.
        let clock = MarketClock::from_utc(utc(2026, 6, 23, 13, 0));
        assert_eq!(clock.session(), Some(MarketSession::PreOpen));
        assert!(clock
            .main_agent_guidance()
            .unwrap()
            .contains("NOT YET OPENED"));
    }

    #[test]
    fn after_close_weekday() {
        // 2026-06-23 (Tue) 21:00 UTC = 17:00 EDT → after the 16:00 close.
        let clock = MarketClock::from_utc(utc(2026, 6, 23, 21, 0));
        assert_eq!(clock.session(), Some(MarketSession::AfterClose));
        assert!(clock
            .main_agent_guidance()
            .unwrap()
            .contains("CLOSED for the day"));
    }

    #[test]
    fn weekend_closed() {
        // 2026-06-20 is a Saturday — any time of day is the weekend.
        let clock = MarketClock::from_utc(utc(2026, 6, 20, 16, 0));
        assert_eq!(clock.session(), Some(MarketSession::Weekend));
        assert!(clock.main_agent_guidance().unwrap().contains("weekend"));
    }

    #[test]
    fn dst_aware_eastern_offset() {
        // Winter: 2026-01-06 (Tue) 14:00 UTC = 09:00 EST (UTC-5) → before the open.
        let winter = MarketClock::from_utc(utc(2026, 1, 6, 14, 0));
        assert_eq!(winter.session(), Some(MarketSession::PreOpen));
        // Summer: 2026-06-23 (Tue) 14:00 UTC = 10:00 EDT (UTC-4) → open.
        let summer = MarketClock::from_utc(utc(2026, 6, 23, 14, 0));
        assert_eq!(summer.session(), Some(MarketSession::Open));
    }

    #[test]
    fn session_boundaries_open_inclusive_close_exclusive() {
        // Exactly 9:30 ET → open. 2026-06-23 13:30 UTC = 09:30 EDT.
        assert_eq!(
            MarketClock::from_utc(utc(2026, 6, 23, 13, 30)).session(),
            Some(MarketSession::Open),
        );
        // One minute before close → still open. 19:59 UTC = 15:59 EDT.
        assert_eq!(
            MarketClock::from_utc(utc(2026, 6, 23, 19, 59)).session(),
            Some(MarketSession::Open),
        );
        // Exactly 16:00 ET → closed. 2026-06-23 20:00 UTC = 16:00 EDT.
        assert_eq!(
            MarketClock::from_utc(utc(2026, 6, 23, 20, 0)).session(),
            Some(MarketSession::AfterClose),
        );
    }

    #[test]
    fn report_date_is_the_eastern_calendar_date() {
        // 16:04 UTC = 12:04 EDT — same ET calendar day.
        assert_eq!(
            MarketClock::from_utc(utc(2026, 6, 23, 16, 4))
                .report_date()
                .as_deref(),
            Some("2026-06-23"),
        );
        // 2026-06-24 02:30 UTC = 2026-06-23 22:30 EDT — the ET trading day is the 23rd.
        assert_eq!(
            MarketClock::from_utc(utc(2026, 6, 24, 2, 30))
                .report_date()
                .as_deref(),
            Some("2026-06-23"),
        );
        assert!(MarketClock::default().report_date().is_none());
    }

    #[test]
    fn default_clock_yields_no_block() {
        assert_eq!(MarketClock::default().session(), None);
        assert!(MarketClock::default().main_agent_guidance().is_none());
    }

    #[test]
    fn et_session_date_rolls_back_across_utc_midnight() {
        // 2026-08-05 01:30 UTC = 2026-08-04 21:30 EDT — the ET session day is the 4th.
        assert_eq!(
            et_session_date(utc(2026, 8, 5, 1, 30)),
            NaiveDate::from_ymd_opt(2026, 8, 4).unwrap(),
        );
        // Winter (EST, UTC-5): 2026-01-07 03:30 UTC = 2026-01-06 22:30 EST.
        assert_eq!(
            et_session_date(utc(2026, 1, 7, 3, 30)),
            NaiveDate::from_ymd_opt(2026, 1, 6).unwrap(),
        );
        // A daytime instant stays on its own date.
        assert_eq!(
            et_session_date(utc(2026, 6, 23, 16, 4)),
            NaiveDate::from_ymd_opt(2026, 6, 23).unwrap(),
        );
    }

    #[test]
    fn et_stamp_labels_the_et_session_and_wall_clock() {
        // 2026-08-05 01:30 UTC = 2026-08-04 21:30 EDT: the label stays on the ET session.
        assert_eq!(et_stamp(utc(2026, 8, 5, 1, 30)), "2026-08-04 21:30 ET");
        // Winter (EST, UTC-5): 2026-01-07 03:30 UTC = 2026-01-06 22:30 EST.
        assert_eq!(et_stamp(utc(2026, 1, 7, 3, 30)), "2026-01-06 22:30 ET");
        // A morning instant zero-pads its hour.
        assert_eq!(et_stamp(utc(2026, 6, 23, 13, 5)), "2026-06-23 09:05 ET");
    }

    #[test]
    fn et_date_of_exotic_forms_pin_the_frontend_parity_contract() {
        // The same cases pin the frontend mirror (src/etDate.ts,
        // tests/etDate.test.ts): chrono's RFC3339 parse accepts space / 't'
        // separators, lowercase 'z', fractional seconds and the :60 leap second,
        // and rejects hour 24+, minute 60, second 61+ and a colon-less offset.
        let et = |s: &str| et_date_of(s).map(|d| d.to_string());
        assert_eq!(et("2026-08-06 01:30:00+00:00").as_deref(), Some("2026-08-05"));
        assert_eq!(et("2026-08-06t01:30:00+00:00").as_deref(), Some("2026-08-05"));
        assert_eq!(et("2026-08-06T01:30:00z").as_deref(), Some("2026-08-05"));
        assert_eq!(
            et("2026-08-06T01:30:00.123+00:00").as_deref(),
            Some("2026-08-05"),
        );
        assert_eq!(
            et("2026-08-06T01:30:60+00:00").as_deref(),
            Some("2026-08-05"),
            "the :60 leap second converts, not prefix-degrades",
        );
        // The hour-24 case uses a negative offset so conversion (08-06) and
        // prefix would disagree: the prefix read proves the parse rejected it.
        assert_eq!(et("2026-08-05T24:00:00-06:00").as_deref(), Some("2026-08-05"));
        assert_eq!(et("2026-08-06T25:00:00+00:00").as_deref(), Some("2026-08-06"));
        assert_eq!(et("2026-08-06T01:60:00+00:00").as_deref(), Some("2026-08-06"));
        assert_eq!(et("2026-08-06T01:30:61+00:00").as_deref(), Some("2026-08-06"));
        assert_eq!(et("2026-08-06T01:30:00+0000").as_deref(), Some("2026-08-06"));
    }

    /// The same table `tests/etDate.test.ts` pins on the frontend mirror.
    pub(super) const CALENDAR_MONTH_CASES: [(&str, u32, &str); 9] = [
        ("2026-01-05", 3, "2026-04-05"),
        ("2026-01-05", 12, "2027-01-05"),
        ("2026-01-05", 36, "2029-01-05"),
        ("2025-11-30", 3, "2026-02-28"),
        ("2026-01-31", 1, "2026-02-28"),
        ("2027-11-30", 3, "2028-02-29"),
        ("2024-02-29", 12, "2025-02-28"),
        ("2026-08-31", 3, "2026-11-30"),
        ("2026-12-15", 3, "2027-03-15"),
    ];

    #[test]
    fn calendar_months_add_and_clamp_on_the_pinned_cases() {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        for (from, months, to) in CALENDAR_MONTH_CASES {
            assert_eq!(add_calendar_months(d(from), months), Some(d(to)), "{from} + {months}");
        }
    }

    #[test]
    fn et_date_of_parses_instants_dates_and_degrades() {
        assert_eq!(
            et_date_of("2026-08-05T01:30:00+00:00"),
            NaiveDate::from_ymd_opt(2026, 8, 4),
        );
        assert_eq!(
            et_date_of("2026-08-05"),
            NaiveDate::from_ymd_opt(2026, 8, 5),
        );
        assert_eq!(
            et_date_of("2026-08-05T99:99:99"),
            NaiveDate::from_ymd_opt(2026, 8, 5),
        );
        assert_eq!(et_date_of("soon"), None);
        assert_eq!(et_date_of(""), None);
    }
}
