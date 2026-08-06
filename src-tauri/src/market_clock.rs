//! Market session state: where the US equity market stands relative to its regular
//! trading hours at the moment a report's baseline was gathered.
//!
//! Reports are generated **on demand**, at any hour (`docs/scheduling.md §Generating a
//! Report`). The baseline scan stamps an `as_of` instant (`pipeline.rs`), but the agents
//! previously received only the *cadence* (how long since the prior report) — never the
//! wall-clock time or whether the market was open. A report generated mid-session then
//! narrated the day in the past tense ("fell 0.95% on the session", "closed green")
//! because nothing told it the session was still live. This type closes that gap: it
//! classifies `as_of` against NYSE/Nasdaq regular hours (9:30 AM–4:00 PM
//! America/New_York, Mon–Fri) and yields a posture block the main agent uses to get the
//! tense right.
//!
//! It is the time-of-day sibling of [`crate::cadence::ReportCadence`] (elapsed interval):
//! both are pure, app-layer classifications threaded into the agent prompt, deliberately
//! kept out of the agents themselves so the spine stays unit-testable.
//!
//! Scope: the Eastern offset is DST-correct via `chrono-tz`, but market **holidays** and
//! half-days are out of scope — a holiday classifies as a normal weekday. The cost of a
//! rare wrong "open" label on a holiday is small next to a holiday table that needs yearly
//! maintenance; revisit if it proves misleading in practice.

use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc, Weekday};
use chrono_tz::America::New_York;
use chrono_tz::Tz;

/// The US/Eastern calendar date of a UTC instant — the market's session date.
///
/// Every persisted run/vintage instant is UTC RFC3339, but the trading day it
/// belongs to is the ET day: an evening-ET run (after ~8 PM EDT / 7 PM EST) has
/// already rolled to the next UTC date, so truncating the UTC string to its date
/// prefix dates the run one session late. Everything that keys a vintage,
/// evidence boundary, entry anchor, or basis bridge to a session dates through
/// here (`docs/portfolio-analysis.md` §Outcome learning, §The quick check).
pub fn et_session_date(utc: DateTime<Utc>) -> NaiveDate {
    utc.with_timezone(&New_York).date_naive()
}

/// The ET session date of a persisted timestamp string.
///
/// A full RFC3339 instant converts to its US/Eastern calendar date; a string
/// that does not parse as an instant falls back to its bare `YYYY-MM-DD` prefix
/// (a date-only value carries no instant to convert, and a malformed timestamp
/// degrades to the old prefix read rather than vanishing); `None` when neither
/// parses.
pub fn et_date_of(stamp: &str) -> Option<NaiveDate> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(stamp) {
        return Some(dt.with_timezone(&New_York).date_naive());
    }
    stamp
        .get(..10)
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
}

/// Regular-session open, minutes since ET midnight (9:30 AM).
const SESSION_OPEN_MINUTES: u32 = 9 * 60 + 30;
/// Regular-session close, minutes since ET midnight (4:00 PM).
const SESSION_CLOSE_MINUTES: u32 = 16 * 60;
/// Regular-session length in hours (6.5h), used to caption progress through the day.
const SESSION_HOURS: f64 = 6.5;

/// Where `as_of` falls relative to the US equity market's regular trading hours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketSession {
    /// A weekday before the 9:30 AM ET open — the session has not started.
    PreOpen,
    /// Within regular trading hours (9:30 AM–4:00 PM ET) — the session is live.
    Open,
    /// A weekday after the 4:00 PM ET close — the session is finished.
    AfterClose,
    /// Saturday or Sunday — closed for the weekend.
    Weekend,
}

/// The resolved session context for a run, or its absence.
#[derive(Debug, Clone)]
struct Snapshot {
    /// `as_of` rendered in US/Eastern (the market's timezone), DST-correct.
    et: DateTime<Tz>,
    session: MarketSession,
    /// Minutes since ET midnight — cached so the Open caption needn't recompute.
    minutes: u32,
}

/// The market-session state at the moment a run's baseline was gathered. `Default` is
/// the no-context state (the offline/stub path, which has no real `as_of`): it carries no
/// snapshot, so [`Self::main_agent_guidance`] yields `None` and the prompt omits the
/// block — mirroring how [`crate::cadence::ReportCadence`]'s default is the first-report
/// case. Cloned (not `Copy`) because it holds a `DateTime`; it is consumed once into
/// `MainAgentInput`.
#[derive(Debug, Clone, Default)]
pub struct MarketClock {
    snapshot: Option<Snapshot>,
}

impl MarketClock {
    /// Classify the baseline's `as_of` instant against US/Eastern regular trading hours.
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

    /// The classified session, or `None` on the no-context (offline/stub) path.
    pub fn session(&self) -> Option<MarketSession> {
        self.snapshot.as_ref().map(|s| s.session)
    }

    /// The run's report date in market (US/Eastern) time as `YYYY-MM-DD`, or `None` on
    /// the no-context default. This is the recency anchor handed to the news-selection
    /// stages (the headline filter and the research router) so "current / prior day" is
    /// measured against a real date rather than guessed from the spread of publish dates —
    /// the gap Codex flagged where those stages were told to favor recency with nothing to
    /// measure it against. ET (the market day), so a late-evening Pacific run still anchors
    /// to the correct trading date.
    pub fn report_date(&self) -> Option<String> {
        self.snapshot
            .as_ref()
            .map(|s| s.et.format("%Y-%m-%d").to_string())
    }

    /// The market-session block appended to the main agent's prompt: it states the
    /// wall-clock ET time and the market's state, and tells the Head Market Analyst the
    /// **tense** to narrate the day in — present/provisional while the session is open
    /// (today's move is intraday vs the prior close, not final), past tense once it has
    /// closed. Always begins with the `Market session:` marker so the wiring is
    /// assertable. `None` on the no-context path, so the prompt omits the block.
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

    /// Build a UTC instant for a test. June 2026 is EDT (UTC-4); January 2026 is EST
    /// (UTC-5) — the two seasons exercise the DST handling.
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
        // The same UTC wall-clock (14:00) lands in different ET sessions by season,
        // which only holds if the Eastern offset is DST-correct.
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
        // 2026-06-24 02:30 UTC = 2026-06-23 22:30 EDT — a late-evening ET run anchors to
        // the ET trading day (the 23rd), not the UTC day, which is what news recency wants.
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
        // The offline/stub path has no real `as_of`, so the block is omitted entirely.
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
    fn et_date_of_exotic_forms_pin_the_frontend_parity_contract() {
        // The grammar the frontend mirrors byte-for-byte (src/etDate.ts,
        // tests/etDate.test.ts — same cases): chrono's RFC3339 parse accepts
        // space / 't' separators, lowercase 'z', fractional seconds, and the
        // :60 leap second — each converting to the ET day (one earlier inside
        // the evening window) — and rejects hour 24+, minute 60, second 61+,
        // and a colon-less offset, degrading to the date prefix.
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
        // Rejected instants degrade to the prefix — the shared fallback. The
        // hour-24 case uses a negative offset so conversion and prefix would
        // disagree (conversion would read 08-06): the prefix read proves the
        // parse rejected it.
        assert_eq!(et("2026-08-05T24:00:00-06:00").as_deref(), Some("2026-08-05"));
        assert_eq!(et("2026-08-06T25:00:00+00:00").as_deref(), Some("2026-08-06"));
        assert_eq!(et("2026-08-06T01:60:00+00:00").as_deref(), Some("2026-08-06"));
        assert_eq!(et("2026-08-06T01:30:61+00:00").as_deref(), Some("2026-08-06"));
        assert_eq!(et("2026-08-06T01:30:00+0000").as_deref(), Some("2026-08-06"));
    }

    #[test]
    fn et_date_of_parses_instants_dates_and_degrades() {
        // A full RFC3339 instant converts through the ET offset.
        assert_eq!(
            et_date_of("2026-08-05T01:30:00+00:00"),
            NaiveDate::from_ymd_opt(2026, 8, 4),
        );
        // A date-only string carries no instant — taken as-is.
        assert_eq!(
            et_date_of("2026-08-05"),
            NaiveDate::from_ymd_opt(2026, 8, 5),
        );
        // A malformed timestamp degrades to its date prefix rather than vanishing.
        assert_eq!(
            et_date_of("2026-08-05T99:99:99"),
            NaiveDate::from_ymd_opt(2026, 8, 5),
        );
        // Garbage parses as nothing.
        assert_eq!(et_date_of("soon"), None);
        assert_eq!(et_date_of(""), None);
    }
}
