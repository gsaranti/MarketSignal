//! The economic-calendar consensus and surprise join (`docs/data-sources.md §Report
//! enrichment`): FMP's paid `economic-calendar` layered onto FRED's release schedule.
//! The wire fetch lives in `fmp` behind [`EconomicCalendarSource`]; the join runs inside
//! the FRED adapter's calendar loop. Fail-soft throughout: an unmapped release, a null
//! estimate or an empty fetch leaves the release at names and dates — never a
//! fabricated consensus.

use anyhow::{anyhow, Result};
use chrono::{Duration, NaiveDate};
use serde_json::Value;

use crate::data_sources::{
    BeatMiss, CalendarEventClass, DataGap, EconomicRelease, EconomicSurprise, SurpriseRelation,
};

/// Which side of the consensus is favorable for an event, so an out-of-band print reads as
/// a beat or a miss. Activity reads are higher-favorable, inflation reads lower-favorable,
/// and prints whose market read inverts by regime (wages, job openings) are neutral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    HigherFavorable,
    LowerFavorable,
    Neutral,
}

/// One release ↔ event pair of the curated map; `event` is the FMP name with its
/// reference period stripped and `epsilon` the in-line band in `unit`.
#[derive(Debug, Clone, Copy)]
pub struct EventMapRow {
    pub release_id: u32,
    pub event: &'static str,
    pub unit: &'static str,
    pub polarity: Polarity,
    pub epsilon: f64,
}

/// The release → event map (`docs/data-sources.md §Report enrichment`). A release's rows
/// form its event set, in the order its `surprises` serialize; a wrong FMP event string
/// fails soft to an unmapped event.
pub const EVENT_MAP: &[EventMapRow] = &[
    row(50, "Non Farm Payrolls", "thousands of jobs", Polarity::HigherFavorable, 25.0),
    row(50, "Unemployment Rate", "percent", Polarity::LowerFavorable, 0.1),
    row(50, "Average Hourly Earnings MoM", MOM, Polarity::Neutral, 0.1),
    row(10, "Inflation Rate MoM", MOM, Polarity::LowerFavorable, 0.1),
    row(10, "Core Inflation Rate MoM", MOM, Polarity::LowerFavorable, 0.1),
    row(10, "Inflation Rate YoY", "% year-over-year", Polarity::LowerFavorable, 0.1),
    row(46, "Producer Price Index MoM", MOM, Polarity::LowerFavorable, 0.2),
    row(46, "Core PPI MoM", MOM, Polarity::LowerFavorable, 0.2),
    row(54, "PCE Price Index MoM", MOM, Polarity::LowerFavorable, 0.1),
    row(54, "Core PCE Price Index MoM", MOM, Polarity::LowerFavorable, 0.1),
    row(54, "Personal Income MoM", MOM, Polarity::HigherFavorable, 0.2),
    row(54, "Personal Spending MoM", MOM, Polarity::HigherFavorable, 0.2),
    row(9, "Retail Sales MoM", MOM, Polarity::HigherFavorable, 0.3),
    row(9, "Retail Sales Ex Autos MoM", MOM, Polarity::HigherFavorable, 0.3),
    row(192, "JOLTs Job Openings", "millions of openings", Polarity::Neutral, 0.2),
    row(53, "GDP Growth Rate QoQ", QOQ_ANNUALIZED, Polarity::HigherFavorable, 0.3),
    row(53, "GDP Price Index QoQ", QOQ_ANNUALIZED, Polarity::LowerFavorable, 0.2),
];

const MOM: &str = "% month-over-month";
const QOQ_ANNUALIZED: &str = "% annualized quarter-over-quarter";

const fn row(
    release_id: u32,
    event: &'static str,
    unit: &'static str,
    polarity: Polarity,
    epsilon: f64,
) -> EventMapRow {
    EventMapRow {
        release_id,
        event,
        unit,
        polarity,
        epsilon,
    }
}

/// The Fed / FOMC events carried as their own calendar class, matched exactly on the
/// period-stripped FMP name. The feed's Fed-speaker rows and its bare `Press
/// Conference` row on the decision date stay out.
pub const FED_EVENTS: &[&str] = &[
    "Fed Interest Rate Decision",
    "FOMC Minutes",
    "FOMC Economic Projections",
];

/// The FMP `impact` levels a Fed / FOMC event must carry to be appended.
const FED_IMPACTS: &[&str] = &["Medium", "High"];

/// How far an FMP print's date may sit from the FRED release date and still join — one
/// shared rule for a cross-midnight publish (FMP serves UTC datetimes).
pub const DATE_TOLERANCE_DAYS: i64 = 1;

/// Float slack on the in-line comparison, so a print exactly one epsilon off the
/// estimate (`4.2` vs `4.1` at `±0.1`) reads in-line despite binary rounding.
const EPSILON_SLACK: f64 = 1e-9;

/// One FMP economic-calendar row, read leniently: `event` as served (reference period
/// included), `date` the calendar date of the served datetime, `datetime` kept for
/// latest-print ordering.
#[derive(Debug, Clone, PartialEq)]
pub struct CalendarRow {
    pub event: String,
    pub date: NaiveDate,
    pub datetime: String,
    pub impact: Option<String>,
    pub estimate: Option<f64>,
    pub actual: Option<f64>,
}

impl CalendarRow {
    /// Read one row of the provider JSON. `None` for a row with no event name or no
    /// parseable date; a value that is not a finite number (or a numeric string) reads as
    /// absent.
    fn from_value(value: &Value) -> Option<Self> {
        let event = value.get("event").and_then(Value::as_str)?.trim().to_string();
        if event.is_empty() {
            return None;
        }
        let datetime = value.get("date").and_then(Value::as_str)?.trim().to_string();
        let date = NaiveDate::parse_from_str(datetime.get(..10)?, "%Y-%m-%d").ok()?;
        Some(Self {
            event,
            date,
            datetime,
            impact: value
                .get("impact")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_string()),
            estimate: lenient_number(value.get("estimate")),
            actual: lenient_number(value.get("actual")),
        })
    }
}

fn lenient_number(value: Option<&Value>) -> Option<f64> {
    let n = match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }?;
    n.is_finite().then_some(n)
}

/// Whether a provider row is tagged with a country other than the US — skipped on
/// purpose, never counted as unreadable.
fn is_foreign(value: &Value) -> bool {
    value
        .get("country")
        .and_then(Value::as_str)
        .is_some_and(|c| c.trim() != "US")
}

/// Shape an `economic-calendar` response into rows. The body must be a JSON array. A row
/// tagged with another country is skipped, and so is a row that cannot be read while
/// others can; a response whose rows are all unreadable is an error, so a feed whose
/// shape changed reads as malformed rather than as a quiet window.
pub fn rows_from_value(value: Value) -> Result<Vec<CalendarRow>> {
    let Value::Array(items) = value else {
        return Err(anyhow!("FMP economic-calendar response was not a JSON array"));
    };
    let mut rows = Vec::with_capacity(items.len());
    let mut unreadable = 0usize;
    for item in items.iter().filter(|item| !is_foreign(item)) {
        match CalendarRow::from_value(item) {
            Some(row) => rows.push(row),
            None => unreadable += 1,
        }
    }
    if rows.is_empty() && unreadable > 0 {
        return Err(anyhow!(
            "none of the {unreadable} FMP economic-calendar rows could be read"
        ));
    }
    Ok(rows)
}

/// The FMP event name with its trailing parenthesized reference period stripped
/// (`Non Farm Payrolls (Jun)` → `Non Farm Payrolls`, `Initial Jobless Claims (Jul/04)` →
/// `Initial Jobless Claims`); a name with no trailing period is returned trimmed.
pub fn strip_period(event: &str) -> &str {
    let trimmed = event.trim();
    trimmed
        .strip_suffix(')')
        .and_then(|inner| inner.rfind('(').map(|open| inner[..open].trim_end()))
        .unwrap_or(trimmed)
}

/// The latest print (by served datetime, the later feed row on a tie) of `event` dated
/// within `pred`.
fn latest<'a>(
    rows: &'a [CalendarRow],
    event: &str,
    pred: impl Fn(&CalendarRow) -> bool,
) -> Option<&'a CalendarRow> {
    rows.iter()
        .filter(|r| strip_period(&r.event) == event && pred(r))
        .fold(None, |best: Option<&CalendarRow>, r| match best {
            Some(b) if b.datetime > r.datetime => Some(b),
            _ => Some(r),
        })
}

/// The `surprises` for one FRED release date: per map row of `release_id`, the latest
/// matching print within [`DATE_TOLERANCE_DAYS`] that carries an estimate, classified. An
/// unmapped release, or a print with a null estimate, yields no entry; the release's other
/// events are unaffected.
pub fn surprises_for(
    release_id: u32,
    release_date: NaiveDate,
    rows: &[CalendarRow],
) -> Vec<EconomicSurprise> {
    EVENT_MAP
        .iter()
        .filter(|m| m.release_id == release_id)
        .filter_map(|m| {
            let print = latest(rows, m.event, |r| {
                (r.date - release_date).num_days().abs() <= DATE_TOLERANCE_DAYS
            })?;
            Some(classify(m, print.estimate?, print.actual))
        })
        .collect()
}

/// Classify one print against its consensus: the relation tag (`in-line` within the
/// row's epsilon), the absolute gap in the event's units, the signed percent gap (null on
/// a zero estimate) and the polarity-mapped beat / miss (null in-line or with no
/// polarity). An unreleased print carries the estimate alone.
pub fn classify(row: &EventMapRow, estimate: f64, actual: Option<f64>) -> EconomicSurprise {
    let mut out = EconomicSurprise {
        event: row.event.to_string(),
        unit: row.unit.to_string(),
        estimate,
        actual,
        relation: None,
        abs_gap: None,
        gap_pct: None,
        beat_miss: None,
    };
    let Some(actual) = actual else {
        return out;
    };
    let diff = actual - estimate;
    let relation = if diff.abs() <= row.epsilon + EPSILON_SLACK {
        SurpriseRelation::InLine
    } else if diff > 0.0 {
        SurpriseRelation::Above
    } else {
        SurpriseRelation::Below
    };
    out.relation = Some(relation);
    out.abs_gap = Some(diff.abs());
    out.gap_pct = (estimate != 0.0).then(|| diff / estimate.abs() * 100.0);
    out.beat_miss = match (relation, row.polarity) {
        (SurpriseRelation::InLine, _) | (_, Polarity::Neutral) => None,
        (SurpriseRelation::Above, Polarity::HigherFavorable)
        | (SurpriseRelation::Below, Polarity::LowerFavorable) => Some(BeatMiss::Beat),
        (SurpriseRelation::Above, Polarity::LowerFavorable)
        | (SurpriseRelation::Below, Polarity::HigherFavorable) => Some(BeatMiss::Miss),
    };
    out
}

/// The Fed / FOMC events dated within `[from, to]`: the [`FED_EVENTS`] names at Medium or
/// High impact, one entry per event and date (the latest row winning), ordered by time
/// then name.
/// Each is `released` before `today` (the scan's ET session date) and `upcoming` from it.
pub fn fed_events(
    rows: &[CalendarRow],
    from: NaiveDate,
    to: NaiveDate,
    today: NaiveDate,
) -> Vec<EconomicRelease> {
    let mut picked: Vec<&CalendarRow> = Vec::new();
    for r in rows {
        let name = strip_period(&r.event);
        let in_class = FED_EVENTS.contains(&name)
            && r.impact.as_deref().is_some_and(|i| FED_IMPACTS.contains(&i))
            && r.date >= from
            && r.date <= to;
        if !in_class {
            continue;
        }
        match picked
            .iter_mut()
            .find(|p| strip_period(&p.event) == name && p.date == r.date)
        {
            Some(p) if p.datetime <= r.datetime => *p = r,
            Some(_) => {}
            None => picked.push(r),
        }
    }
    picked.sort_by(|a, b| {
        (a.datetime.as_str(), strip_period(&a.event)).cmp(&(b.datetime.as_str(), strip_period(&b.event)))
    });
    picked
        .into_iter()
        .map(|r| EconomicRelease {
            release: strip_period(&r.event).to_string(),
            date: r.date.format("%Y-%m-%d").to_string(),
            status: if r.date < today { "released" } else { "upcoming" }.to_string(),
            class: CalendarEventClass::Fed,
            surprises: Vec::new(),
        })
        .collect()
}

/// The mapped events of `release_ids` (the releases with a FRED date in the window) that
/// no fetched row carries anywhere in the window, by period-stripped name, in map order.
/// Empty when the fetch returned no rows. A non-empty result is the map-drift signal: a
/// renamed FMP event fails soft to an unmatched release, so this is what shows it.
pub fn absent_events(release_ids: &[u32], rows: &[CalendarRow]) -> Vec<&'static str> {
    if rows.is_empty() {
        return Vec::new();
    }
    EVENT_MAP
        .iter()
        .filter(|m| release_ids.contains(&m.release_id))
        .filter(|m| !rows.iter().any(|r| strip_period(&r.event) == m.event))
        .map(|m| m.event)
        .collect()
}

/// The consensus feed the FRED calendar joins against — FMP's paid economic calendar on
/// the report path, a canned source in tests.
pub trait EconomicCalendarSource: Send + Sync {
    /// The calendar rows dated `[from, to]`. Empty when the feed is unreachable this run
    /// (the calendar then stays at names and dates); a this-run failure records its gap
    /// into `gaps`, a plan-tier refusal records none.
    fn economic_calendar(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        gaps: &mut Vec<DataGap>,
    ) -> Vec<CalendarRow>;
}

/// The fetch window for a FRED calendar window `[start, end]`: widened by the date
/// tolerance on both sides so a print published across midnight still joins.
pub fn fetch_window(start: NaiveDate, end: NaiveDate) -> (NaiveDate, NaiveDate) {
    let pad = Duration::days(DATE_TOLERANCE_DAYS);
    (start - pad, end + pad)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn print(event: &str, datetime: &str, estimate: Option<f64>, actual: Option<f64>) -> CalendarRow {
        CalendarRow {
            event: event.into(),
            date: d(&datetime[..10]),
            datetime: datetime.into(),
            impact: Some("High".into()),
            estimate,
            actual,
        }
    }

    fn map_row(event: &str) -> &'static EventMapRow {
        EVENT_MAP.iter().find(|m| m.event == event).unwrap()
    }

    #[test]
    fn strip_period_removes_only_the_trailing_reference_token() {
        assert_eq!(strip_period("Non Farm Payrolls (Jun)"), "Non Farm Payrolls");
        assert_eq!(strip_period("GDP Growth Rate QoQ (Q1)"), "GDP Growth Rate QoQ");
        assert_eq!(strip_period("Initial Jobless Claims (Jul/04)"), "Initial Jobless Claims");
        assert_eq!(strip_period("FOMC Minutes"), "FOMC Minutes");
        assert_eq!(strip_period("  Retail Sales MoM (Sep) "), "Retail Sales MoM");
    }

    #[test]
    fn a_matched_release_classifies_each_component_print_by_its_own_polarity() {
        // Employment Situation fans out to three events with different units and
        // polarities: payrolls below consensus is a miss, unemployment below is a beat,
        // and wages carry no beat / miss at all.
        let rows = vec![
            print("Non Farm Payrolls (Sep)", "2026-10-02 12:30:00", Some(90.0), Some(29.0)),
            print("Unemployment Rate (Sep)", "2026-10-02 12:30:00", Some(4.3), Some(4.1)),
            print("Average Hourly Earnings MoM (Sep)", "2026-10-02 12:30:00", Some(0.3), Some(0.5)),
        ];
        let s = surprises_for(50, d("2026-10-02"), &rows);
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].event, "Non Farm Payrolls");
        assert_eq!(s[0].unit, "thousands of jobs");
        assert_eq!(s[0].relation, Some(SurpriseRelation::Below));
        assert_eq!(s[0].abs_gap, Some(61.0));
        assert!((s[0].gap_pct.unwrap() - (-61.0 / 90.0 * 100.0)).abs() < 1e-9);
        assert_eq!(s[0].beat_miss, Some(BeatMiss::Miss));
        assert_eq!(s[1].relation, Some(SurpriseRelation::Below));
        assert_eq!(s[1].beat_miss, Some(BeatMiss::Beat));
        assert_eq!(s[2].relation, Some(SurpriseRelation::Above));
        assert_eq!(s[2].beat_miss, None, "wages carry no favorable polarity");
    }

    #[test]
    fn an_unmapped_release_carries_no_surprises() {
        let rows = vec![print("Non Farm Payrolls (Sep)", "2026-10-02 12:30:00", Some(90.0), Some(29.0))];
        assert!(surprises_for(999, d("2026-10-02"), &rows).is_empty());
        // So does a mapped release with nothing on its date.
        assert!(surprises_for(10, d("2026-10-02"), &rows).is_empty());
    }

    #[test]
    fn a_null_estimate_drops_that_event_alone() {
        let rows = vec![
            print("PCE Price Index MoM (Aug)", "2026-09-30 12:30:00", None, Some(0.3)),
            print("Core PCE Price Index MoM (Aug)", "2026-09-30 12:30:00", Some(0.3), Some(0.2)),
        ];
        let s = surprises_for(54, d("2026-09-30"), &rows);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].event, "Core PCE Price Index MoM");
        assert_eq!(s[0].relation, Some(SurpriseRelation::InLine), "0.1 off at ±0.1 is in-line");
        assert_eq!(s[0].beat_miss, None);
    }

    #[test]
    fn an_upcoming_print_carries_its_consensus_alone() {
        let rows = vec![print("GDP Growth Rate QoQ (Q3)", "2026-10-29 12:30:00", Some(0.9), None)];
        let s = surprises_for(53, d("2026-10-29"), &rows);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].estimate, 0.9);
        assert_eq!(
            (s[0].actual, s[0].relation, s[0].abs_gap, s[0].gap_pct, s[0].beat_miss),
            (None, None, None, None, None)
        );
    }

    #[test]
    fn a_zero_estimate_leaves_the_percent_gap_null_but_carries_the_absolute_gap() {
        let s = classify(map_row("Retail Sales MoM"), 0.0, Some(-0.6));
        assert_eq!(s.relation, Some(SurpriseRelation::Below));
        assert_eq!(s.abs_gap, Some(0.6));
        assert_eq!(s.gap_pct, None);
        assert_eq!(s.beat_miss, Some(BeatMiss::Miss));
    }

    #[test]
    fn the_in_line_band_is_inclusive_at_the_epsilon_despite_float_rounding() {
        let unemployment = map_row("Unemployment Rate");
        // 4.2 − 4.1 is 0.10000000000000053 in binary; still in-line at ±0.1.
        assert_eq!(classify(unemployment, 4.1, Some(4.2)).relation, Some(SurpriseRelation::InLine));
        assert_eq!(classify(unemployment, 4.1, Some(4.3)).relation, Some(SurpriseRelation::Above));
        let payrolls = map_row("Non Farm Payrolls");
        assert_eq!(classify(payrolls, 100.0, Some(125.0)).relation, Some(SurpriseRelation::InLine));
        let beat = classify(payrolls, 100.0, Some(126.0));
        assert_eq!((beat.relation, beat.beat_miss), (Some(SurpriseRelation::Above), Some(BeatMiss::Beat)));
    }

    #[test]
    fn near_names_and_alias_rows_never_collide_with_a_mapped_event() {
        // The CPI release date carries the level-index and NSA rows beside the mapped
        // prints; only the exact stripped names join.
        let rows = vec![
            print("CPI s.a (Aug)", "2026-09-11 12:30:00", Some(334.14), Some(334.131)),
            print("CPI n.s.a MoM (Aug)", "2026-09-11 12:30:00", Some(0.3), Some(0.32)),
            print("Cleveland CPI MoM (Aug)", "2026-09-11 15:00:00", Some(0.2), Some(0.2)),
            print("Inflation Rate (Aug)", "2026-09-11 12:30:00", Some(334.85), Some(334.98)),
            print("Core Inflation Rate (Aug)", "2026-09-11 12:30:00", Some(337.0), Some(337.77)),
            print("Inflation Rate MoM (Aug)", "2026-09-11 12:30:00", Some(0.4), Some(0.4)),
            print("Core Inflation Rate MoM (Aug)", "2026-09-11 12:30:00", Some(0.2), Some(0.3)),
            print("Inflation Rate YoY (Aug)", "2026-09-11 12:30:00", Some(3.4), Some(3.4)),
            print("Core Inflation Rate YoY (Aug)", "2026-09-11 12:30:00", Some(2.4), Some(2.4)),
        ];
        let s = surprises_for(10, d("2026-09-11"), &rows);
        let events: Vec<&str> = s.iter().map(|e| e.event.as_str()).collect();
        assert_eq!(events, ["Inflation Rate MoM", "Core Inflation Rate MoM", "Inflation Rate YoY"]);
        assert_eq!(s[0].estimate, 0.4, "the MoM row, not the level index");
        assert_eq!(s[1].beat_miss, None, "0.1 above at ±0.1 is in-line");
        // The U-6 rate never stands in for the headline rate.
        let u6 = vec![print("U-6 Unemployment Rate (Sep)", "2026-10-02 12:30:00", Some(8.0), Some(8.4))];
        assert!(surprises_for(50, d("2026-10-02"), &u6).is_empty());
    }

    #[test]
    fn duplicate_prints_resolve_to_the_latest() {
        let rows = vec![
            print("Retail Sales MoM (Aug)", "2026-09-16 12:30:00", Some(0.8), Some(1.2)),
            print("Retail Sales MoM (Aug)", "2026-09-16 14:00:00", Some(0.8), Some(0.9)),
            print("Retail Sales MoM (Aug)", "2026-09-16 12:00:00", Some(0.8), Some(2.0)),
        ];
        let s = surprises_for(9, d("2026-09-16"), &rows);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].actual, Some(0.9));
    }

    #[test]
    fn the_date_tolerance_joins_a_day_off_and_refuses_two() {
        let next_day = vec![print("JOLTs Job Openings (Aug)", "2026-09-30 00:30:00", Some(7.23), Some(7.079))];
        let s = surprises_for(192, d("2026-09-29"), &next_day);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].unit, "millions of openings");
        assert_eq!(s[0].relation, Some(SurpriseRelation::InLine));
        let two_off = vec![print("JOLTs Job Openings (Aug)", "2026-10-01 12:30:00", Some(7.23), Some(7.079))];
        assert!(surprises_for(192, d("2026-09-29"), &two_off).is_empty());
    }

    #[test]
    fn fed_events_take_the_allowlisted_names_at_medium_or_high_impact() {
        let mut low = print("FOMC Minutes", "2026-08-19 18:00:00", None, None);
        low.impact = Some("Low".into());
        let rows = vec![
            print("Fed Interest Rate Decision", "2026-10-28 18:00:00", None, None),
            print("FOMC Minutes", "2026-10-07 18:00:00", None, None),
            print("FOMC Economic Projections", "2026-09-16 18:00:00", None, None),
            print("Fed Interest Rate Decision", "2026-09-16 18:00:00", Some(4.0), Some(4.0)),
            print("Press Conference", "2026-09-16 18:30:00", None, None),
            print("Fed Waller Speech", "2026-10-13 07:45:00", None, None),
            print("Interest Rate Projection - Current", "2026-09-16 18:00:00", None, None),
            low,
            // Outside the window.
            print("FOMC Minutes", "2026-11-18 19:00:00", None, None),
        ];
        let fed = fed_events(&rows, d("2026-09-01"), d("2026-11-02"), d("2026-10-12"));
        let got: Vec<(&str, &str, &str)> = fed
            .iter()
            .map(|e| (e.release.as_str(), e.date.as_str(), e.status.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("FOMC Economic Projections", "2026-09-16", "released"),
                ("Fed Interest Rate Decision", "2026-09-16", "released"),
                ("FOMC Minutes", "2026-10-07", "released"),
                ("Fed Interest Rate Decision", "2026-10-28", "upcoming"),
            ]
        );
        assert!(fed.iter().all(|e| e.class == CalendarEventClass::Fed && e.surprises.is_empty()));
    }

    #[test]
    fn a_duplicated_fed_event_appears_once() {
        let rows = vec![
            print("FOMC Minutes", "2026-10-07 18:00:00", None, None),
            print("FOMC Minutes", "2026-10-07 18:00:00", None, None),
        ];
        assert_eq!(fed_events(&rows, d("2026-10-01"), d("2026-10-31"), d("2026-10-07")).len(), 1);
        // The session date itself reads as upcoming, like the FRED rows.
        assert_eq!(fed_events(&rows, d("2026-10-01"), d("2026-10-31"), d("2026-10-07"))[0].status, "upcoming");
    }

    #[test]
    fn rows_parse_leniently_and_skip_what_cannot_be_read() {
        let value = json!([
            {"date": "2026-10-02 12:30:00", "country": "US", "event": "Non Farm Payrolls (Sep)",
             "impact": "High", "estimate": 90, "actual": 29, "unit": "K"},
            {"date": "2026-10-02 12:30:00", "country": "US", "event": "Unemployment Rate (Sep)",
             "impact": "High", "estimate": "4.1", "actual": null},
            {"date": "2026-10-02 12:30:00", "country": "CA", "event": "Unemployment Rate (Sep)",
             "estimate": 6.9, "actual": 7.1},
            {"date": "not a date", "event": "Retail Sales MoM (Sep)"},
            {"date": "2026-10-02 12:30:00", "event": ""},
            {"date": "2026-10-02 12:30:00", "event": "Personal Income MoM (Aug)", "estimate": "n/a"},
            "not an object"
        ]);
        let rows = rows_from_value(value).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].estimate, Some(90.0));
        assert_eq!(rows[0].date, d("2026-10-02"));
        assert_eq!((rows[1].estimate, rows[1].actual), (Some(4.1), None), "a numeric string reads");
        assert_eq!(rows[2].estimate, None, "an unreadable number is absent, the row kept");
        assert!(rows_from_value(json!({"Error Message": "x"})).is_err());
    }

    #[test]
    fn a_response_with_no_readable_row_is_malformed_but_an_empty_or_foreign_one_is_not() {
        // Every row unreadable: the feed's shape changed, not a quiet window.
        let unreadable = json!([
            {"country": "US", "event": "Non Farm Payrolls", "date": "invalid"},
            {"country": "US", "name": "Retail Sales MoM", "date": "2026-10-15 12:30:00"}
        ]);
        assert!(rows_from_value(unreadable).is_err());
        // Unreadable US rows beside non-US ones are still all unreadable.
        let with_foreign = json!([
            {"country": "CA", "event": "Unemployment Rate", "date": "2026-10-10 12:30:00"},
            {"country": "US", "event": "Non Farm Payrolls", "date": "invalid"}
        ]);
        assert!(rows_from_value(with_foreign).is_err());
        // An empty window, or rows all filtered out by country, is empty — not malformed.
        assert!(rows_from_value(json!([])).unwrap().is_empty());
        let foreign = json!([
            {"country": "CA", "event": "Unemployment Rate", "date": "2026-10-10 12:30:00"}
        ]);
        assert!(rows_from_value(foreign).unwrap().is_empty());
    }

    #[test]
    fn every_map_row_carries_a_positive_epsilon_and_a_unique_event_per_release() {
        for (i, m) in EVENT_MAP.iter().enumerate() {
            assert!(m.epsilon > 0.0, "{}", m.event);
            assert_eq!(strip_period(m.event), m.event, "{} is a stripped name", m.event);
            assert!(
                !EVENT_MAP[..i].iter().any(|p| p.event == m.event),
                "{} is mapped twice",
                m.event
            );
        }
    }

    #[test]
    fn absent_events_names_the_mapped_events_no_row_carries() {
        // The feed still serves the CPI prints under their old names: every Consumer
        // Price Index event is absent, while GDP — present on a later date only — is not.
        let rows = vec![
            print("CPI MoM (Aug)", "2026-09-11 12:30:00", Some(0.4), Some(0.4)),
            print("Core CPI MoM (Aug)", "2026-09-11 12:30:00", Some(0.2), Some(0.3)),
            print("GDP Growth Rate QoQ (Q3)", "2026-10-29 12:30:00", Some(0.9), None),
            print("GDP Price Index QoQ (Q3)", "2026-10-29 12:30:00", None, None),
        ];
        assert_eq!(
            absent_events(&[10, 53], &rows),
            ["Inflation Rate MoM", "Core Inflation Rate MoM", "Inflation Rate YoY"]
        );
        // A release with no FRED date in the window is not checked.
        assert!(absent_events(&[53], &rows).is_empty());
        // No rows (a free key, an outage) is no signal.
        assert!(absent_events(&[10, 53], &[]).is_empty());
    }

    #[test]
    fn the_fetch_window_pads_both_edges_by_the_tolerance() {
        assert_eq!(
            fetch_window(d("2026-09-01"), d("2026-10-31")),
            (d("2026-08-31"), d("2026-11-01"))
        );
    }
}
