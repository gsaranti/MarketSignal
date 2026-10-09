//! Outcome learning — the accuracy record (`docs/portfolio-analysis.md §Outcome
//! learning`): an append-only store of **price records** (episodes), each scored
//! once per horizon by an engine-computed **check**, and the per-arm **accuracy
//! scores** derived from the checks. Everything here is deterministic; no model
//! stage is involved, and nothing the model writes alters a check or a score.
//!
//! The core is pure and job-agnostic: the due rule, the check, the score and the
//! opening cadence read a plain [`PriceRecord`] and its checks, so Trade
//! Opportunities' episode store (the same record plus its lifecycle id and
//! decision class) reuses them over its own table. The impure seams are thin:
//! the closes source ([`OutcomePriceSource`]) and the store calls the job makes
//! (`portfolio::store`).

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{Datelike, Months, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

use crate::portfolio::engine::{self, DatedValue};

// ---- Calibratable constants (`docs/portfolio-analysis.md §Starting parameters`) --

/// The episode cadence: a priced holding analyzed this run opens an episode when
/// it has none, or when its latest is this many calendar months old or more.
pub const EPISODE_CADENCE_MONTHS: u32 = 1;

/// The horizon proximity: a check reads the close on the horizon date, or the
/// last session at or before it within this many sessions. Sessions count as
/// weekdays — `market_clock` carries no holiday table, so a holiday counts as a
/// session and the window is at most this many weekdays back.
pub const HORIZON_PROXIMITY_SESSIONS: i64 = 5;

/// The review check lines: the self-review's realized block shows the checks
/// written since the prior analysis, newest by horizon date, up to this many.
pub const REVIEW_CHECK_LINES: usize = 12;

// ---- The record --------------------------------------------------------------

/// The three forecast horizons, each a window from the episode's creation date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Horizon {
    ThreeMonth,
    TwelveMonth,
    ThreeYear,
}

impl Horizon {
    pub const ALL: [Horizon; 3] = [Horizon::ThreeMonth, Horizon::TwelveMonth, Horizon::ThreeYear];

    /// The horizon's length in calendar months.
    pub fn months(self) -> u32 {
        match self {
            Horizon::ThreeMonth => 3,
            Horizon::TwelveMonth => 12,
            Horizon::ThreeYear => 36,
        }
    }

    /// The horizon's stored key — the store's `horizon` column.
    pub fn key(self) -> &'static str {
        match self {
            Horizon::ThreeMonth => "three_month",
            Horizon::TwelveMonth => "twelve_month",
            Horizon::ThreeYear => "three_year",
        }
    }

    /// The horizon a stored key names; `None` for any other text.
    pub fn from_key(key: &str) -> Option<Self> {
        Horizon::ALL.into_iter().find(|h| h.key() == key)
    }
}

/// One value per horizon, each null where none was stated — the model's expected
/// prices (null where the appendix left a horizon unstated) or the engine's base
/// values (null where the engine authored no band).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct HorizonPrices {
    pub three_month: Option<f64>,
    pub twelve_month: Option<f64>,
    pub three_year: Option<f64>,
}

impl HorizonPrices {
    pub fn get(&self, horizon: Horizon) -> Option<f64> {
        match horizon {
            Horizon::ThreeMonth => self.three_month,
            Horizon::TwelveMonth => self.twelve_month,
            Horizon::ThreeYear => self.three_year,
        }
    }

    /// Whether any horizon carries a value.
    pub fn any(&self) -> bool {
        Horizon::ALL.iter().any(|h| self.get(*h).is_some())
    }
}

/// An episode — the **price record**: the symbol, the creation date (the
/// creation run's ET session), the spot that day, the anchor close that bridges
/// the record across a later split (the newest settled close strictly before the
/// creation session, with its bar date), the model's expected share price and
/// the engine's base value at the three horizons — nothing else. Never updated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceRecord {
    pub symbol: String,
    /// ISO `YYYY-MM-DD` — data, never identity (identity is insertion order).
    pub created_on: String,
    pub spot: f64,
    /// `None` when the creation run had no dated closes — every horizon then
    /// writes unscorable rather than comparing cross-basis.
    pub anchor: Option<DatedValue>,
    pub model: HorizonPrices,
    pub engine: HorizonPrices,
}

/// An episode under its store identity — the autoincrement id, whose order is
/// insertion order (which episode is a holding's latest).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredEpisode {
    pub id: i64,
    pub record: PriceRecord,
}

// ---- Checks ------------------------------------------------------------------

/// One arm's leg of a scored check: the expected price as compared — the
/// recorded price converted through the check's bridge factor onto the close's
/// basis — and the per-check score.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LegScore {
    pub expected: f64,
    pub score: f64,
}

/// Why a horizon was written unscorable at once (it leaves the due set and
/// counts in no score).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnscorableCause {
    /// Neither arm recorded a price at this horizon.
    NoForecast,
    /// The episode was created with no dated closes, so its prices carry no
    /// anchor to bridge from.
    NoAnchor,
    /// The refreshed series no longer carries the episode's anchor bar.
    AnchorBarMissing,
    /// The refreshed series served no close inside the proximity — a delisting,
    /// an acquisition, a ticker change, a symbol the source never covered.
    NoCloseInProximity,
}

/// What a check found at its horizon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum CheckOutcome {
    Scored {
        /// The close read — the horizon date's, or the last session at or before
        /// it within the proximity — with its bar date.
        close: DatedValue,
        /// The split-bridge factor the recorded prices converted through (1.0
        /// where the series was not re-based since creation).
        bridge_factor: f64,
        /// `None` where the episode recorded no model price at this horizon.
        model: Option<LegScore>,
        /// `None` where the episode recorded no engine base value at this horizon.
        engine: Option<LegScore>,
    },
    Unscorable { cause: UnscorableCause },
}

/// A check, written onto its episode once: the horizon, the check date (the
/// writing run's ET session), the run that wrote it, and what it found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Check {
    pub horizon: Horizon,
    pub checked_on: String,
    pub run_id: String,
    pub outcome: CheckOutcome,
}

/// A check under its store identity: the autoincrement id (insertion order —
/// run order, which the self-review's read-by-the-prior-analysis word compares)
/// and the episode it scores.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredCheck {
    pub id: i64,
    pub episode_id: i64,
    pub check: Check,
}

/// A horizon's date: the creation date plus the horizon's calendar months (a
/// month-end creation clamps to the target month's last day —
/// [`crate::market_clock::add_calendar_months`]).
pub fn horizon_date(created_on: NaiveDate, horizon: Horizon) -> Option<NaiveDate> {
    crate::market_clock::add_calendar_months(created_on, horizon.months())
}

/// Every due horizon — one whose date is **strictly before** the run's session
/// (so the close it reads has settled) and onto which nothing has been written —
/// as `(episode id, horizon)`, in insertion order then horizon order. One query
/// over the store: nothing is ever re-scored, and a horizon is scored by the
/// first run after its date, however many runs later that is. `written` is
/// every `(episode, horizon)` the store holds a check row for, read or not.
pub fn due_horizons(
    episodes: &[StoredEpisode],
    written: &std::collections::HashSet<(i64, Horizon)>,
    session: NaiveDate,
) -> Vec<(i64, Horizon)> {
    let mut due = Vec::new();
    for ep in episodes {
        let Some(created) = parse_date(&ep.record.created_on) else {
            continue;
        };
        for h in Horizon::ALL {
            let Some(date) = horizon_date(created, h) else {
                continue;
            };
            if date < session && !written.contains(&(ep.id, h)) {
                due.push((ep.id, h));
            }
        }
    }
    due
}

/// The cause a due horizon is unscorable for **without reading any series** —
/// no forecast at the horizon, or no anchor to bridge from — so the symbol
/// spends no pull on it. `None` when the horizon needs the refreshed series.
pub fn unscorable_without_series(record: &PriceRecord, horizon: Horizon) -> Option<UnscorableCause> {
    if record.model.get(horizon).is_none() && record.engine.get(horizon).is_none() {
        return Some(UnscorableCause::NoForecast);
    }
    if record.anchor.is_none() {
        return Some(UnscorableCause::NoAnchor);
    }
    None
}

/// The per-check score: `100 × (1 − |expected − actual| ⁄ actual)`, floored at 0.
pub fn per_check_score(expected: f64, actual: f64) -> f64 {
    (100.0 * (1.0 - (expected - actual).abs() / actual)).max(0.0)
}

/// Score one due horizon against the symbol's freshly fetched dated-EOD series.
/// The anchor bar and the horizon close are both read from `closes`, one fetch,
/// so they sit on one basis; the recorded prices cross the split-adjustment
/// bridge (`engine::split_bridge_factor`, its deadband included) before scoring,
/// so a split can never score as a miss.
pub fn check_horizon(record: &PriceRecord, horizon: Horizon, closes: &[DatedValue]) -> CheckOutcome {
    let unscorable = |cause| CheckOutcome::Unscorable { cause };
    if let Some(cause) = unscorable_without_series(record, horizon) {
        return unscorable(cause);
    }
    let Some(target) = parse_date(&record.created_on).and_then(|d| horizon_date(d, horizon)) else {
        return unscorable(UnscorableCause::NoCloseInProximity);
    };
    let Some(close) = close_within_proximity(closes, target) else {
        return unscorable(UnscorableCause::NoCloseInProximity);
    };
    let anchor = record.anchor.as_ref().expect("checked above");
    let Some(factor) = engine::split_bridge_factor(closes, anchor) else {
        return unscorable(UnscorableCause::AnchorBarMissing);
    };
    let leg = |price: Option<f64>| {
        price.map(|p| {
            let expected = p * factor;
            LegScore {
                expected,
                score: per_check_score(expected, close.value),
            }
        })
    };
    CheckOutcome::Scored {
        close: close.clone(),
        bridge_factor: factor,
        model: leg(record.model.get(horizon)),
        engine: leg(record.engine.get(horizon)),
    }
}

/// The close a check reads: the last usable bar (finite, positive) at or before
/// the horizon date, provided no more than [`HORIZON_PROXIMITY_SESSIONS`]
/// sessions lie after it up to and including the horizon date.
fn close_within_proximity(closes: &[DatedValue], target: NaiveDate) -> Option<&DatedValue> {
    let (bar, date) = closes
        .iter()
        .filter(|b| b.value.is_finite() && b.value > 0.0)
        .filter_map(|b| parse_date(&b.date).map(|d| (b, d)))
        .filter(|(_, d)| *d <= target)
        .max_by_key(|(_, d)| *d)?;
    (sessions_after(date, target) <= HORIZON_PROXIMITY_SESSIONS).then_some(bar)
}

/// The weekdays strictly after `from` up to and including `to`.
fn sessions_after(from: NaiveDate, to: NaiveDate) -> i64 {
    let mut n = 0;
    let mut d = from;
    while d < to {
        d = d.succ_opt().expect("in range");
        if !matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
            n += 1;
        }
    }
    n
}

pub(crate) fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

// ---- Opening -----------------------------------------------------------------

/// Whether a priced subject analyzed this run opens an episode: it has none, or
/// its latest (by insertion order) is [`EPISODE_CADENCE_MONTHS`] or more old at
/// the run's session. A subject re-priced between those points is not recorded,
/// so the scored forecast is the monthly snapshot.
pub fn cadence_opens(latest_created_on: Option<&str>, session: NaiveDate) -> bool {
    let Some(latest) = latest_created_on else {
        return true;
    };
    match parse_date(latest).and_then(|d| d.checked_add_months(Months::new(EPISODE_CADENCE_MONTHS)))
    {
        Some(due) => due <= session,
        // An unreadable creation date cannot hold the cadence closed.
        None => true,
    }
}

// ---- The accuracy scores -----------------------------------------------------

/// One arm's accuracy score at one horizon: the arithmetic mean of its per-check
/// scores, 0 to 100, over the checks that scored the arm, with the check that
/// last moved it — its check date and its store id (run order).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArmScore {
    pub score: f64,
    pub checks: u32,
    pub last_moved_on: String,
    pub last_moved_check: i64,
}

/// Both arms' scores at one horizon — `None` is "no score yet".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HorizonAccuracy {
    pub model: Option<ArmScore>,
    pub engine: Option<ArmScore>,
}

/// A subject's six accuracy scores — per horizon, per arm.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AccuracyScores {
    pub three_month: HorizonAccuracy,
    pub twelve_month: HorizonAccuracy,
    pub three_year: HorizonAccuracy,
}

impl AccuracyScores {
    pub fn at(&self, horizon: Horizon) -> &HorizonAccuracy {
        match horizon {
            Horizon::ThreeMonth => &self.three_month,
            Horizon::TwelveMonth => &self.twelve_month,
            Horizon::ThreeYear => &self.three_year,
        }
    }

    fn at_mut(&mut self, horizon: Horizon) -> &mut HorizonAccuracy {
        match horizon {
            Horizon::ThreeMonth => &mut self.three_month,
            Horizon::TwelveMonth => &mut self.twelve_month,
            Horizon::ThreeYear => &mut self.three_year,
        }
    }
}

/// Derive a subject's accuracy scores from its checks (any order): an
/// unscorable check counts in no score, and a scored check counts for each arm
/// whose leg it carries — a null model price's check scores the engine alone.
pub fn accuracy_scores<'a>(checks: impl IntoIterator<Item = &'a StoredCheck>) -> AccuracyScores {
    #[derive(Default)]
    struct Acc {
        sum: f64,
        n: u32,
        last: Option<(i64, String)>,
    }
    impl Acc {
        fn add(&mut self, leg: &LegScore, check: &StoredCheck) {
            self.sum += leg.score;
            self.n += 1;
            if self.last.as_ref().is_none_or(|(id, _)| check.id > *id) {
                self.last = Some((check.id, check.check.checked_on.clone()));
            }
        }
        fn finish(self) -> Option<ArmScore> {
            let (id, on) = self.last?;
            Some(ArmScore {
                score: self.sum / self.n as f64,
                checks: self.n,
                last_moved_on: on,
                last_moved_check: id,
            })
        }
    }
    let mut accs: BTreeMap<Horizon, (Acc, Acc)> = BTreeMap::new();
    for c in checks {
        if let CheckOutcome::Scored { model, engine, .. } = &c.check.outcome {
            let (m, e) = accs.entry(c.check.horizon).or_default();
            if let Some(leg) = model {
                m.add(leg, c);
            }
            if let Some(leg) = engine {
                e.add(leg, c);
            }
        }
    }
    let mut out = AccuracyScores::default();
    for (h, (m, e)) in accs {
        *out.at_mut(h) = HorizonAccuracy {
            model: m.finish(),
            engine: e.finish(),
        };
    }
    out
}

/// Each symbol's accuracy scores over the whole store — every episode of the
/// symbol, held or exited, since the record scores the forecast, not the book.
pub fn scores_by_symbol(
    episodes: &[StoredEpisode],
    checks: &[StoredCheck],
    symbols: impl IntoIterator<Item = String>,
) -> BTreeMap<String, AccuracyScores> {
    let symbol_of: std::collections::HashMap<i64, String> = episodes
        .iter()
        .map(|e| (e.id, e.record.symbol.to_ascii_uppercase()))
        .collect();
    symbols
        .into_iter()
        .map(|s| {
            let key = s.to_ascii_uppercase();
            let scores = accuracy_scores(
                checks
                    .iter()
                    .filter(|c| symbol_of.get(&c.episode_id) == Some(&key)),
            );
            (key, scores)
        })
        .collect()
}

impl ArmScore {
    /// Whether the prior analysis read this score as it stands: the check that
    /// last moved it was written at or before the mark the prior pass's review
    /// recorded (`docs/portfolio-analysis.md` §Outcome learning — run order,
    /// not the calendar). No mark — a prior that read no accuracy record —
    /// read nothing.
    pub fn read_by(&self, read_through: Option<i64>) -> bool {
        read_through.is_some_and(|mark| self.last_moved_check <= mark)
    }
}

/// One subject's accuracy record as the pass leaves it for the subject's
/// self-review: the six scores, the subject's episodes and every check
/// written onto them (this run's included, since the checks precede the
/// loop), and the highest check id in the whole store — the mark a review
/// that reads this record writes. `high_water` is `None` on an empty store.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubjectAccuracy {
    pub scores: AccuracyScores,
    pub episodes: Vec<StoredEpisode>,
    pub checks: Vec<StoredCheck>,
    pub high_water: Option<i64>,
}

/// The subject's slice of the store: its episodes (insertion order), the
/// checks written onto them, its scores, and the whole store's highest check
/// id.
pub fn subject_accuracy(
    episodes: &[StoredEpisode],
    checks: &[StoredCheck],
    symbol: &str,
) -> SubjectAccuracy {
    let key = symbol.to_ascii_uppercase();
    let own: Vec<StoredEpisode> = episodes
        .iter()
        .filter(|e| e.record.symbol.eq_ignore_ascii_case(&key))
        .cloned()
        .collect();
    let ids: std::collections::HashSet<i64> = own.iter().map(|e| e.id).collect();
    let own_checks: Vec<StoredCheck> = checks
        .iter()
        .filter(|c| ids.contains(&c.episode_id))
        .cloned()
        .collect();
    SubjectAccuracy {
        scores: accuracy_scores(&own_checks),
        episodes: own,
        checks: own_checks,
        high_water: checks.iter().map(|c| c.id).max(),
    }
}

/// The checks the self-review lists: those written after the prior pass's
/// mark (every check where the prior read none), newest by horizon date —
/// ties by check id, newest first — each beside its episode, capped at
/// `cap`, with the total before the cap.
#[derive(Debug, Clone, PartialEq)]
pub struct ChecksSince<'a> {
    pub lines: Vec<(&'a StoredEpisode, &'a StoredCheck, NaiveDate)>,
    pub total: usize,
}

pub fn checks_since(subject: &SubjectAccuracy, read_through: Option<i64>, cap: usize) -> ChecksSince<'_> {
    let episode_of: std::collections::HashMap<i64, &StoredEpisode> =
        subject.episodes.iter().map(|e| (e.id, e)).collect();
    let mut lines: Vec<(&StoredEpisode, &StoredCheck, NaiveDate)> = subject
        .checks
        .iter()
        .filter(|c| read_through.is_none_or(|mark| c.id > mark))
        .filter_map(|c| {
            let ep = *episode_of.get(&c.episode_id)?;
            let date = parse_date(&ep.record.created_on).and_then(|d| horizon_date(d, c.check.horizon))?;
            Some((ep, c, date))
        })
        .collect();
    lines.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.id.cmp(&a.1.id)));
    let total = lines.len();
    lines.truncate(cap);
    ChecksSince { lines, total }
}

/// The run's accuracy record (`docs/storage.md §Local Analysis Suite Storage`
/// — the run audit's accuracy records): each holding's accuracy scores, keyed by
/// symbol, for every holding in the run — carried and failed ones included,
/// since their checks ran this run too; the episodes this run opened; and the
/// checks and unscorable states this run wrote (by run id, so a resumed run's
/// list spans both processes).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AccuracyRecord {
    /// `None` when the episode store could not be read this run — the scores
    /// are unavailable, not "no score yet" (the run's data health counts the gap).
    pub scores: Option<BTreeMap<String, AccuracyScores>>,
    pub opened: Vec<StoredEpisode>,
    pub checks: Vec<StoredCheck>,
}

// ---- The closes source -------------------------------------------------------

/// Where a check's dated-EOD series comes from — the refresh the accuracy pass
/// writes through the shared price-bar cache.
pub trait OutcomePriceSource {
    fn daily_closes(&self, symbol: &str, from: NaiveDate, to: NaiveDate)
        -> Result<Vec<DatedValue>>;
}

/// The live source: FMP dated EOD, the suite's one deep-price rung.
pub struct LiveOutcomePrices {
    pub fmp: crate::fmp::FmpDataSource,
}

impl OutcomePriceSource for LiveOutcomePrices {
    fn daily_closes(
        &self,
        symbol: &str,
        from: NaiveDate,
        _to: NaiveDate,
    ) -> Result<Vec<DatedValue>> {
        // The FMP dated-EOD fetch is now-anchored; a lookback from today
        // covering `from` spans the requested range, which is why the range's
        // own upper bound goes unread here.
        // A lookback COUNT, not a session key: the UTC date is fine here —
        // one extra day of history is harmless, and the bars are then
        // selected by their own dates.
        // An honest empty serve (an HTTP-200 `[]`) passes through as a served
        // series with no close: the check writes the horizon unscorable at once
        // and the symbol spends no further pull. Only a failed fetch is `Err`,
        // leaving the horizon pending.
        let today = chrono::Utc::now().date_naive();
        let lookback = (today - from).num_days().max(1);
        self.fmp
            .fetch_dated_eod(symbol, lookback)
            .map_err(|fmp_err| fmp_err.context(format!("FMP dated EOD failed for {symbol}")))
    }
}

/// A source with nothing to serve — every refresh fails, so every due horizon
/// that needs a series stays pending (the posture of a job run with no price
/// source).
pub struct UnavailablePriceSource;

impl OutcomePriceSource for UnavailablePriceSource {
    fn daily_closes(&self, symbol: &str, _: NaiveDate, _: NaiveDate) -> Result<Vec<DatedValue>> {
        anyhow::bail!("no outcome price source available ({symbol})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn bar(date: &str, value: f64) -> DatedValue {
        DatedValue {
            date: date.to_string(),
            value,
        }
    }

    /// Weekday closes from `from` through `to` at a constant value.
    fn weekday_closes(from: &str, to: &str, value: f64) -> Vec<DatedValue> {
        let mut out = Vec::new();
        let mut day = d(from);
        while day <= d(to) {
            if !matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
                out.push(bar(&day.format("%Y-%m-%d").to_string(), value));
            }
            day = day.succ_opt().unwrap();
        }
        out
    }

    fn prices(m3: Option<f64>, m12: Option<f64>, m36: Option<f64>) -> HorizonPrices {
        HorizonPrices {
            three_month: m3,
            twelve_month: m12,
            three_year: m36,
        }
    }

    fn record(created_on: &str) -> PriceRecord {
        PriceRecord {
            symbol: "AAPL".into(),
            created_on: created_on.into(),
            spot: 100.0,
            anchor: Some(bar("2026-01-02", 100.0)),
            model: prices(Some(110.0), Some(120.0), Some(150.0)),
            engine: prices(Some(105.0), Some(115.0), Some(140.0)),
        }
    }

    fn stored(id: i64, record: PriceRecord) -> StoredEpisode {
        StoredEpisode { id, record }
    }

    fn scored_check(
        id: i64,
        episode_id: i64,
        horizon: Horizon,
        on: &str,
        model: Option<f64>,
        engine: Option<f64>,
    ) -> StoredCheck {
        let leg = |s: Option<f64>| s.map(|score| LegScore { expected: 1.0, score });
        StoredCheck {
            id,
            episode_id,
            check: Check {
                horizon,
                checked_on: on.into(),
                run_id: "run".into(),
                outcome: CheckOutcome::Scored {
                    close: bar(on, 1.0),
                    bridge_factor: 1.0,
                    model: leg(model),
                    engine: leg(engine),
                },
            },
        }
    }

    #[test]
    fn horizon_dates_add_calendar_months_and_clamp_month_end() {
        assert_eq!(horizon_date(d("2026-01-05"), Horizon::ThreeMonth), Some(d("2026-04-05")));
        assert_eq!(horizon_date(d("2026-01-05"), Horizon::TwelveMonth), Some(d("2027-01-05")));
        assert_eq!(horizon_date(d("2026-01-05"), Horizon::ThreeYear), Some(d("2029-01-05")));
        // A month-end creation clamps to the target month's last day.
        assert_eq!(horizon_date(d("2025-11-30"), Horizon::ThreeMonth), Some(d("2026-02-28")));
    }

    #[test]
    fn a_horizon_is_due_only_after_its_date_and_until_written() {
        let eps = vec![stored(1, record("2026-01-05"))];
        let none = std::collections::HashSet::new();
        // On the horizon date itself: not due (that session's close is unsettled).
        assert!(due_horizons(&eps, &none, d("2026-04-05")).is_empty());
        // The session after: the three-month horizon is due.
        assert_eq!(due_horizons(&eps, &none, d("2026-04-06")), vec![(1, Horizon::ThreeMonth)]);
        // Written once, it is never due again.
        let written = std::collections::HashSet::from([(1, Horizon::ThreeMonth)]);
        assert!(due_horizons(&eps, &written, d("2026-05-01")).is_empty());
        // Long after: every horizon past its date is due, in horizon order.
        assert_eq!(
            due_horizons(&eps, &none, d("2029-02-01")),
            vec![(1, Horizon::ThreeMonth), (1, Horizon::TwelveMonth), (1, Horizon::ThreeYear)]
        );
        // The stored keys name their horizons.
        for h in Horizon::ALL {
            assert_eq!(Horizon::from_key(h.key()), Some(h));
        }
        assert_eq!(Horizon::from_key("one_month"), None);
    }

    #[test]
    fn the_per_check_score_is_one_minus_the_relative_error_floored_at_zero() {
        assert_eq!(per_check_score(100.0, 100.0), 100.0);
        assert!((per_check_score(110.0, 100.0) - 90.0).abs() < 1e-9);
        assert!((per_check_score(90.0, 100.0) - 90.0).abs() < 1e-9);
        // An error larger than the close floors at zero, never negative.
        assert_eq!(per_check_score(250.0, 100.0), 0.0);
    }

    #[test]
    fn a_check_scores_both_arms_against_the_horizon_close() {
        let rec = record("2026-01-05");
        let closes = weekday_closes("2026-01-02", "2026-04-10", 100.0);
        match check_horizon(&rec, Horizon::ThreeMonth, &closes) {
            CheckOutcome::Scored { close, bridge_factor, model, engine } => {
                // 2026-04-05 is a Sunday: the last session at or before it is Friday.
                assert_eq!(close.date, "2026-04-03");
                assert_eq!(bridge_factor, 1.0);
                assert!((model.unwrap().score - 90.0).abs() < 1e-9);
                assert!((engine.unwrap().score - 95.0).abs() < 1e-9);
            }
            other => panic!("expected a scored check, got {other:?}"),
        }
    }

    #[test]
    fn the_proximity_admits_five_sessions_back_and_not_six() {
        let rec = record("2026-01-09"); // three-month horizon: Thursday 2026-04-09
        let mut closes = vec![bar("2026-01-02", 100.0)];
        // The last close five sessions back (Thursday 2026-04-02): admitted.
        closes.push(bar("2026-04-02", 100.0));
        assert!(matches!(
            check_horizon(&rec, Horizon::ThreeMonth, &closes),
            CheckOutcome::Scored { .. }
        ));
        // Six sessions back (Wednesday 2026-04-01): unscorable.
        closes.pop();
        closes.push(bar("2026-04-01", 100.0));
        assert_eq!(
            check_horizon(&rec, Horizon::ThreeMonth, &closes),
            CheckOutcome::Unscorable {
                cause: UnscorableCause::NoCloseInProximity
            }
        );
    }

    #[test]
    fn a_split_bridges_so_it_scores_as_a_hit_never_a_miss() {
        // A 2:1 split since creation: the refreshed series carries the anchor
        // bar at half its recorded close.
        let rec = record("2026-01-05");
        let mut closes = weekday_closes("2026-01-02", "2026-04-10", 55.0);
        closes[0] = bar("2026-01-02", 50.0);
        match check_horizon(&rec, Horizon::ThreeMonth, &closes) {
            CheckOutcome::Scored { bridge_factor, model, .. } => {
                assert_eq!(bridge_factor, 0.5);
                let model = model.unwrap();
                assert_eq!(model.expected, 55.0);
                assert_eq!(model.score, 100.0);
            }
            other => panic!("expected a scored check, got {other:?}"),
        }
        // A sub-deadband revision of the anchor bar never poses as a split.
        let mut revised = weekday_closes("2026-01-02", "2026-04-10", 110.0);
        revised[0] = bar("2026-01-02", 101.0);
        match check_horizon(&rec, Horizon::ThreeMonth, &revised) {
            CheckOutcome::Scored { bridge_factor, model, .. } => {
                assert_eq!(bridge_factor, 1.0);
                assert_eq!(model.unwrap().score, 100.0);
            }
            other => panic!("expected a scored check, got {other:?}"),
        }
    }

    #[test]
    fn each_unscorable_cause_is_typed() {
        let closes = weekday_closes("2026-01-02", "2026-04-10", 100.0);
        let mut no_forecast = record("2026-01-05");
        no_forecast.model.three_month = None;
        no_forecast.engine.three_month = None;
        assert_eq!(
            check_horizon(&no_forecast, Horizon::ThreeMonth, &closes),
            CheckOutcome::Unscorable { cause: UnscorableCause::NoForecast }
        );
        assert_eq!(unscorable_without_series(&no_forecast, Horizon::ThreeMonth), Some(UnscorableCause::NoForecast));

        let mut no_anchor = record("2026-01-05");
        no_anchor.anchor = None;
        assert_eq!(unscorable_without_series(&no_anchor, Horizon::ThreeMonth), Some(UnscorableCause::NoAnchor));
        assert_eq!(
            check_horizon(&no_anchor, Horizon::ThreeMonth, &closes),
            CheckOutcome::Unscorable { cause: UnscorableCause::NoAnchor }
        );

        let rec = record("2026-01-05");
        assert_eq!(unscorable_without_series(&rec, Horizon::ThreeMonth), None);
        let missing_anchor_bar = weekday_closes("2026-01-05", "2026-04-10", 100.0);
        assert_eq!(
            check_horizon(&rec, Horizon::ThreeMonth, &missing_anchor_bar),
            CheckOutcome::Unscorable { cause: UnscorableCause::AnchorBarMissing }
        );

        // A series that stops long before the horizon (a delisting).
        let delisted = weekday_closes("2026-01-02", "2026-02-27", 100.0);
        assert_eq!(
            check_horizon(&rec, Horizon::ThreeMonth, &delisted),
            CheckOutcome::Unscorable { cause: UnscorableCause::NoCloseInProximity }
        );
    }

    #[test]
    fn a_null_leg_scores_the_other_arm_alone() {
        let closes = weekday_closes("2026-01-02", "2026-04-10", 100.0);
        let mut model_null = record("2026-01-05");
        model_null.model.three_month = None;
        match check_horizon(&model_null, Horizon::ThreeMonth, &closes) {
            CheckOutcome::Scored { model, engine, .. } => {
                assert!(model.is_none());
                assert!(engine.is_some());
            }
            other => panic!("expected a scored check, got {other:?}"),
        }
        let mut engine_null = record("2026-01-05");
        engine_null.engine.three_month = None;
        match check_horizon(&engine_null, Horizon::ThreeMonth, &closes) {
            CheckOutcome::Scored { model, engine, .. } => {
                assert!(model.is_some());
                assert!(engine.is_none());
            }
            other => panic!("expected a scored check, got {other:?}"),
        }
    }

    #[test]
    fn scores_average_per_arm_and_carry_the_check_that_last_moved_them() {
        let checks = vec![
            scored_check(3, 1, Horizon::ThreeMonth, "2026-04-06", Some(80.0), Some(60.0)),
            scored_check(7, 2, Horizon::ThreeMonth, "2026-05-06", None, Some(100.0)),
            StoredCheck {
                id: 9,
                episode_id: 3,
                check: Check {
                    horizon: Horizon::ThreeMonth,
                    checked_on: "2026-06-06".into(),
                    run_id: "run".into(),
                    outcome: CheckOutcome::Unscorable { cause: UnscorableCause::NoCloseInProximity },
                },
            },
        ];
        let s = accuracy_scores(&checks);
        let model = s.three_month.model.as_ref().unwrap();
        assert_eq!(model.score, 80.0);
        assert_eq!(model.checks, 1);
        assert_eq!((model.last_moved_on.as_str(), model.last_moved_check), ("2026-04-06", 3));
        let engine = s.three_month.engine.as_ref().unwrap();
        assert_eq!(engine.score, 80.0);
        assert_eq!(engine.checks, 2);
        // The unscorable check moves nothing.
        assert_eq!((engine.last_moved_on.as_str(), engine.last_moved_check), ("2026-05-06", 7));
        // No score yet where no check landed.
        assert_eq!(s.twelve_month, HorizonAccuracy::default());
        assert!(s.three_year.model.is_none());
    }

    #[test]
    fn scores_by_symbol_reads_every_episode_of_the_symbol() {
        let mut msft = record("2026-01-05");
        msft.symbol = "MSFT".into();
        let eps = vec![stored(1, record("2026-01-05")), stored(2, msft), stored(3, record("2026-02-05"))];
        let checks = vec![
            scored_check(1, 1, Horizon::ThreeMonth, "2026-04-06", Some(80.0), Some(70.0)),
            scored_check(2, 2, Horizon::ThreeMonth, "2026-04-06", Some(10.0), Some(10.0)),
            scored_check(3, 3, Horizon::ThreeMonth, "2026-05-06", Some(60.0), Some(50.0)),
        ];
        let by = scores_by_symbol(&eps, &checks, ["aapl".to_string(), "NVDA".to_string()]);
        assert_eq!(by["AAPL"].three_month.model.as_ref().unwrap().score, 70.0);
        assert_eq!(by["AAPL"].three_month.engine.as_ref().unwrap().checks, 2);
        assert_eq!(by["NVDA"], AccuracyScores::default());
        assert!(!by.contains_key("MSFT"));
    }

    #[test]
    fn the_cadence_opens_at_one_month_and_not_before() {
        assert!(cadence_opens(None, d("2026-03-01")));
        assert!(!cadence_opens(Some("2026-02-05"), d("2026-03-04")));
        assert!(cadence_opens(Some("2026-02-05"), d("2026-03-05")));
        assert!(cadence_opens(Some("2026-01-31"), d("2026-02-28")));
        assert!(!cadence_opens(Some("2026-01-31"), d("2026-02-27")));
    }

    #[test]
    fn the_live_source_passes_an_empty_serve_through_and_errs_only_on_a_failed_fetch() {
        use crate::test_http::{Canned, MockHttp};
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: "[]",
            },
            Canned::Reply {
                status: 404,
                headers: vec![],
                body: "{}",
            },
        ]);
        let live = LiveOutcomePrices {
            fmp: crate::fmp::FmpDataSource::new("test-key".to_string())
                .unwrap()
                .with_base_url(&server.base_url),
        };
        let served = live.daily_closes("GONE", d("2026-01-01"), d("2026-10-09")).unwrap();
        assert!(served.is_empty(), "an empty serve is a served series, never an error");
        let rec = record("2026-01-05");
        assert_eq!(
            check_horizon(&rec, Horizon::ThreeMonth, &served),
            CheckOutcome::Unscorable {
                cause: UnscorableCause::NoCloseInProximity
            }
        );
        assert!(live.daily_closes("GONE", d("2026-01-01"), d("2026-10-09")).is_err());
    }

    #[test]
    fn the_record_round_trips_with_its_kebab_and_snake_tags() {
        let check = Check {
            horizon: Horizon::TwelveMonth,
            checked_on: "2027-01-06".into(),
            run_id: "r".into(),
            outcome: CheckOutcome::Unscorable { cause: UnscorableCause::AnchorBarMissing },
        };
        let json = serde_json::to_value(&check).unwrap();
        assert_eq!(json["horizon"], "twelve_month");
        assert_eq!(json["outcome"]["status"], "unscorable");
        assert_eq!(json["outcome"]["cause"], "anchor-bar-missing");
        assert_eq!(serde_json::from_value::<Check>(json).unwrap(), check);
        let rec = record("2026-01-05");
        let back: PriceRecord = serde_json::from_str(&serde_json::to_string(&rec).unwrap()).unwrap();
        assert_eq!(back, rec);
    }

    #[test]
    fn a_subjects_slice_carries_its_own_checks_and_the_whole_stores_mark() {
        let mut other = record("2026-01-05");
        other.symbol = "MSFT".into();
        let eps = vec![stored(1, record("2026-01-05")), stored(2, other)];
        let checks = vec![
            scored_check(10, 1, Horizon::ThreeMonth, "2026-04-06", Some(90.0), Some(80.0)),
            scored_check(11, 2, Horizon::ThreeMonth, "2026-04-06", Some(50.0), None),
        ];
        let s = subject_accuracy(&eps, &checks, "aapl");
        assert_eq!(s.episodes.len(), 1);
        assert_eq!(s.checks.iter().map(|c| c.id).collect::<Vec<_>>(), vec![10]);
        assert_eq!(s.scores.three_month.model.as_ref().unwrap().score, 90.0);
        // The mark is the whole store's highest check id, another symbol's included.
        assert_eq!(s.high_water, Some(11));
        assert_eq!(subject_accuracy(&[], &[], "AAPL").high_water, None);
    }

    #[test]
    fn the_read_word_compares_the_last_moving_check_with_the_prior_mark() {
        let arm = ArmScore {
            score: 80.0,
            checks: 2,
            last_moved_on: "2026-04-06".into(),
            last_moved_check: 7,
        };
        assert!(arm.read_by(Some(7)));
        assert!(arm.read_by(Some(9)));
        assert!(!arm.read_by(Some(6)));
        // A prior that read no accuracy record read nothing.
        assert!(!arm.read_by(None));
    }

    #[test]
    fn checks_since_the_mark_list_newest_horizon_first_capped_with_the_total() {
        let eps = vec![stored(1, record("2026-01-05")), stored(2, record("2026-02-05"))];
        let checks = vec![
            scored_check(3, 1, Horizon::ThreeMonth, "2026-04-06", Some(90.0), Some(80.0)),
            scored_check(4, 2, Horizon::ThreeMonth, "2026-05-06", Some(70.0), Some(60.0)),
            scored_check(5, 1, Horizon::TwelveMonth, "2027-01-06", Some(60.0), Some(50.0)),
        ];
        let s = subject_accuracy(&eps, &checks, "AAPL");
        // No mark: every check is new, newest horizon date first.
        let all = checks_since(&s, None, REVIEW_CHECK_LINES);
        assert_eq!(all.total, 3);
        assert_eq!(all.lines.iter().map(|(_, c, _)| c.id).collect::<Vec<_>>(), vec![5, 4, 3]);
        assert_eq!(all.lines[0].2, d("2027-01-05"));
        // A mark leaves the checks it covers out.
        let since = checks_since(&s, Some(3), REVIEW_CHECK_LINES);
        assert_eq!(since.lines.iter().map(|(_, c, _)| c.id).collect::<Vec<_>>(), vec![5, 4]);
        // The cap trims the oldest horizons and keeps the total.
        let capped = checks_since(&s, None, 2);
        assert_eq!(capped.total, 3);
        assert_eq!(capped.lines.iter().map(|(_, c, _)| c.id).collect::<Vec<_>>(), vec![5, 4]);
        // A horizon-date tie orders by check id, newest first.
        let tied = vec![
            scored_check(8, 1, Horizon::ThreeMonth, "2026-04-06", Some(1.0), None),
            scored_check(9, 1, Horizon::ThreeMonth, "2026-04-06", Some(1.0), None),
        ];
        let s = subject_accuracy(&eps, &tied, "AAPL");
        let t = checks_since(&s, None, REVIEW_CHECK_LINES);
        assert_eq!(t.lines.iter().map(|(_, c, _)| c.id).collect::<Vec<_>>(), vec![9, 8]);
        // Everything read: none landed.
        assert_eq!(checks_since(&s, Some(9), REVIEW_CHECK_LINES).total, 0);
    }
}
