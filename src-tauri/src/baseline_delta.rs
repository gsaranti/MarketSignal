//! Deterministic change view between two baseline scans.
//!
//! Continuity across reports is otherwise narrative-only (the vector-memory report
//! summaries): the main agent sees this run's numbers and the prior report's prose,
//! but never the prior report's *numbers*. This module closes that gap. The
//! application layer persists each run's baseline (`storage::baseline_snapshots`) and,
//! on the next run, computes the level-by-level change against the most recent prior
//! snapshot here — in Rust, deterministically — so the agent reads an exact, bounded
//! diff rather than re-deriving arithmetic from two raw snapshots.
//!
//! Cadence-honest by construction: reports are not guaranteed weekly (manual runs,
//! missed scheduled runs), so the diff carries the actual elapsed interval
//! ([`BaselineDeltas::elapsed_days`]) rather than assuming a week — a 35 bp move means
//! very different things over an hour versus three weeks.
//!
//! Only **level-bearing** groups are diffed (the [`DELTA_GROUPS`] list): a series whose
//! `price`/`pe`/premium is a standing level, joined by series id. Set-valued or
//! already-derivative groups (movers, earnings, calendar, industries, index
//! performance, and the sector *performance* percentages) are deliberately excluded —
//! diffing a return would be a noisy second derivative. A series present in only one of
//! the two snapshots yields no `SeriesDelta`; it is surfaced as a `new` / `missing`
//! transition instead of a fabricated change.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::data_sources::{BaselineMarketData, GapReason, GroupKind, Quote};

/// The baseline groups whose levels are diffed report-to-report. Level-bearing and
/// joinable by a stable series id; everything else in the baseline is set-valued or
/// already a return, so it carries no meaningful level delta.
pub const DELTA_GROUPS: [GroupKind; 6] = [
    GroupKind::Indices,
    GroupKind::Internals,
    GroupKind::MacroLevels,
    GroupKind::LaborLevels,
    GroupKind::SectorPe,
    GroupKind::MarketRiskPremium,
];

/// Which way a level moved between the prior report and this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    Up,
    Down,
    Flat,
}

/// One series' change between the prior report's baseline and this run's, joined by id.
/// `pct_change` is omitted when the prior level was zero or non-finite (no honest
/// percentage exists) — `abs_change` always carries the move.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeriesDelta {
    pub group: GroupKind,
    pub id: String,
    pub name: String,
    pub current: f64,
    pub prior: f64,
    pub abs_change: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pct_change: Option<f64>,
    pub direction: Direction,
}

/// A series present in exactly one of the two snapshots. No delta is defined, so it is
/// reported as a transition rather than a fabricated change against an implied zero.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeriesTransition {
    pub group: GroupKind,
    pub id: String,
    pub name: String,
    /// Why the series is absent from the run it's missing in, when that run's gap manifest
    /// explains it: a transient `unavailable` / `rejected` / `malformed` fetch failure, or
    /// `out-of-scope`. `None` when no gap recorded the absence — a genuine coverage change,
    /// not a data outage. Lets the agent tell a dropped *data feed* apart from a series
    /// actually entering or leaving the market.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<GapReason>,
}

/// The deterministic change view handed to the agent (and, once Step 8 lands, the
/// research router): how each level-bearing series moved since the previous report,
/// which series newly appeared, and which dropped out — all over an explicit elapsed
/// interval.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BaselineDeltas {
    /// Days elapsed since the prior report's baseline was captured (fractional). The
    /// cadence is not fixed weekly, so this is the real gap, not an assumed week.
    pub elapsed_days: f64,
    /// Series present in both snapshots, with their level change.
    pub changed: Vec<SeriesDelta>,
    /// Series present this run but absent from the prior snapshot.
    pub new: Vec<SeriesTransition>,
    /// Series present in the prior snapshot but absent this run.
    pub missing: Vec<SeriesTransition>,
}

/// `(id, name, level)` for each series in `group` — the uniform shape the join works
/// over. Non-level-bearing groups (everything outside [`DELTA_GROUPS`]) yield nothing.
fn series_levels(data: &BaselineMarketData, group: GroupKind) -> Vec<(String, String, f64)> {
    fn from_quotes(quotes: &[Quote]) -> Vec<(String, String, f64)> {
        quotes
            .iter()
            .map(|q| {
                // The quote name is used verbatim: the typed `change` carries its own unit
                // (`change.kind`), so the baseline name no longer holds a point-delta marker
                // that this cross-report view — self-describing via its own `abs_change` /
                // `pct_change` — would have to strip.
                (q.symbol.clone(), q.name.clone(), q.price)
            })
            .collect()
    }
    match group {
        GroupKind::Indices => from_quotes(&data.indices),
        GroupKind::Internals => from_quotes(&data.internals),
        GroupKind::MacroLevels => from_quotes(&data.macro_levels),
        GroupKind::LaborLevels => from_quotes(&data.labor_levels),
        // Sector P/E is exchange-specific, so the join key pins (sector, exchange) — a
        // NASDAQ tech P/E and an NYSE tech P/E are distinct series. A row whose `pe` was
        // band-dropped to `None` (out of `(0.0, SECTOR_PE_MAX]`) has no level to diff, so it
        // is skipped here — it surfaces as a `new`/`missing` transition the same way a series
        // present in only one run does, rather than diffing against a fabricated level.
        GroupKind::SectorPe => data
            .sector_pe
            .iter()
            .filter_map(|s| {
                s.pe.map(|pe| {
                    (
                        format!("{}|{}", s.sector, s.exchange),
                        format!("{} ({})", s.sector, s.exchange),
                        pe,
                    )
                })
            })
            .collect(),
        GroupKind::MarketRiskPremium => data
            .market_risk_premium
            .iter()
            .map(|m| {
                (
                    m.country.clone(),
                    format!("{} equity risk premium", m.country),
                    m.total_equity_risk_premium,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A `(group, series_id) -> reason` lookup over one snapshot's gap manifest, so a series
/// absent from the gathered levels can be tagged with *why* it was absent. Keyed the way
/// the adapters record gaps (the provider series id, which doubles as the quote symbol);
/// composite-key groups (sector P/E, MRP) simply won't match and degrade to no reason.
fn gap_reasons(data: &BaselineMarketData) -> HashMap<(GroupKind, &str), GapReason> {
    data.gaps
        .iter()
        .map(|g| ((g.group, g.series_id.as_str()), g.reason))
        .collect()
}

/// Compute the level-by-level change of `current` against `prior` over `elapsed_days`.
/// Pure and deterministic: same inputs, same output, no I/O. A series in both snapshots
/// becomes a [`SeriesDelta`]; one present in only `current` is `new`, only `prior` is
/// `missing`. A transition is tagged with the gap reason that explains its absence (the
/// missing run's manifest) when one exists, so a data-feed outage reads differently from
/// a series genuinely entering or leaving the market.
pub fn compute_deltas(
    current: &BaselineMarketData,
    prior: &BaselineMarketData,
    elapsed_days: f64,
) -> BaselineDeltas {
    let mut changed = Vec::new();
    let mut new = Vec::new();
    let mut missing = Vec::new();

    // A series `missing` this run is absent from `current`, so its reason (if any) lives in
    // the current manifest; one that's `new` this run was absent from `prior`, so its prior
    // gap explains whether it newly appeared or merely recovered from a prior outage.
    let current_gaps = gap_reasons(current);
    let prior_gaps = gap_reasons(prior);

    for group in DELTA_GROUPS {
        let cur = series_levels(current, group);
        let pri = series_levels(prior, group);
        let prior_by_id: HashMap<&str, f64> =
            pri.iter().map(|(id, _, v)| (id.as_str(), *v)).collect();
        let current_ids: HashSet<&str> = cur.iter().map(|(id, _, _)| id.as_str()).collect();

        for (id, name, current_level) in &cur {
            match prior_by_id.get(id.as_str()) {
                Some(&prior_level) => {
                    let abs_change = current_level - prior_level;
                    let pct_change = if prior_level != 0.0
                        && prior_level.is_finite()
                        && current_level.is_finite()
                    {
                        Some(abs_change / prior_level * 100.0)
                    } else {
                        None
                    };
                    let direction = if abs_change > 0.0 {
                        Direction::Up
                    } else if abs_change < 0.0 {
                        Direction::Down
                    } else {
                        Direction::Flat
                    };
                    changed.push(SeriesDelta {
                        group,
                        id: id.clone(),
                        name: name.clone(),
                        current: *current_level,
                        prior: prior_level,
                        abs_change,
                        pct_change,
                        direction,
                    });
                }
                None => new.push(SeriesTransition {
                    group,
                    id: id.clone(),
                    name: name.clone(),
                    reason: prior_gaps.get(&(group, id.as_str())).copied(),
                }),
            }
        }

        for (id, name, _) in &pri {
            if !current_ids.contains(id.as_str()) {
                missing.push(SeriesTransition {
                    group,
                    id: id.clone(),
                    name: name.clone(),
                    reason: current_gaps.get(&(group, id.as_str())).copied(),
                });
            }
        }
    }

    BaselineDeltas {
        elapsed_days,
        changed,
        new,
        missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_sources::{Change, MarketRiskPremium, SectorPe};

    fn quote(symbol: &str, name: &str, price: f64) -> Quote {
        Quote {
            symbol: symbol.into(),
            name: name.into(),
            price,
            change: Change::percent(0.0),
            unit: "index points".into(),
        }
    }

    /// rose / fell / new / missing in one pass, plus elapsed pass-through.
    #[test]
    fn computes_rise_fall_new_and_missing() {
        let prior = BaselineMarketData {
            indices: vec![quote("^GSPC", "S&P 500", 5_500.0)],
            internals: vec![
                quote("^VIX", "CBOE Volatility Index", 14.0),
                quote("^TNX", "10-Year Treasury Yield", 4.2),
            ],
            ..Default::default()
        };
        let current = BaselineMarketData {
            indices: vec![quote("^GSPC", "S&P 500", 5_610.0)],
            internals: vec![
                quote("^VIX", "CBOE Volatility Index", 13.0),
                // ^TNX dropped out; ^DXY appeared.
                quote("^DXY", "US Dollar Index", 99.0),
            ],
            ..Default::default()
        };

        let d = compute_deltas(&current, &prior, 6.0);
        assert_eq!(d.elapsed_days, 6.0);

        let sp = d.changed.iter().find(|s| s.id == "^GSPC").unwrap();
        assert!((sp.abs_change - 110.0).abs() < 1e-9);
        assert!((sp.pct_change.unwrap() - 2.0).abs() < 1e-9);
        assert_eq!(sp.direction, Direction::Up);

        let vix = d.changed.iter().find(|s| s.id == "^VIX").unwrap();
        assert!((vix.abs_change + 1.0).abs() < 1e-9);
        assert_eq!(vix.direction, Direction::Down);

        assert!(d.new.iter().any(|t| t.id == "^DXY"));
        assert!(d.missing.iter().any(|t| t.id == "^TNX"));
        // A dropped/added series is never a fabricated change.
        assert!(!d.changed.iter().any(|s| s.id == "^TNX" || s.id == "^DXY"));
    }

    /// The quote name flows into the delta view verbatim. With the typed `change` carrying its
    /// own unit (`change.kind`), the baseline name no longer holds a point-delta marker, so
    /// `from_quotes` no longer strips one — this pins that pass-through (a changed delta and a
    /// `new` transition, the two `from_quotes` outputs) so a future change can't silently
    /// re-mangle the name.
    #[test]
    fn carries_the_quote_name_into_the_delta_view_verbatim() {
        let prior = BaselineMarketData {
            internals: vec![quote("DGS10", "10-Year Treasury Yield", 4.2)],
            ..Default::default()
        };
        let current = BaselineMarketData {
            internals: vec![
                quote("DGS10", "10-Year Treasury Yield", 4.3),
                quote("NFCI", "Chicago Fed NFCI", -0.4),
            ],
            ..Default::default()
        };
        let d = compute_deltas(&current, &prior, 7.0);

        let tnx = d.changed.iter().find(|s| s.id == "DGS10").unwrap();
        assert_eq!(
            tnx.name, "10-Year Treasury Yield",
            "name carried verbatim on a changed delta"
        );
        assert!(
            (tnx.abs_change - 0.1).abs() < 1e-9,
            "the move is still carried by abs_change"
        );

        let nfci = d.new.iter().find(|t| t.id == "NFCI").unwrap();
        assert_eq!(
            nfci.name, "Chicago Fed NFCI",
            "name carried verbatim on a new transition"
        );
    }

    /// A zero prior level yields a move but no percentage (no honest denominator).
    #[test]
    fn zero_prior_level_omits_pct_but_keeps_abs() {
        let prior = BaselineMarketData {
            macro_levels: vec![quote("NETEXP", "Net Exports", 0.0)],
            ..Default::default()
        };
        let current = BaselineMarketData {
            macro_levels: vec![quote("NETEXP", "Net Exports", 0.5)],
            ..Default::default()
        };
        let d = compute_deltas(&current, &prior, 0.04);
        let m = d.changed.iter().find(|s| s.id == "NETEXP").unwrap();
        assert!((m.abs_change - 0.5).abs() < 1e-9);
        assert_eq!(m.pct_change, None);
        assert_eq!(m.direction, Direction::Up);
    }

    /// The same diff over a one-hour gap reports a tiny elapsed honestly — the model,
    /// not this layer, judges whether so small a window makes the move meaningful.
    #[test]
    fn elapsed_passes_through_for_a_rapid_regeneration() {
        let prior = BaselineMarketData {
            indices: vec![quote("^GSPC", "S&P 500", 5_500.0)],
            ..Default::default()
        };
        let current = BaselineMarketData {
            indices: vec![quote("^GSPC", "S&P 500", 5_500.4)],
            ..Default::default()
        };
        let one_hour = 1.0 / 24.0;
        let d = compute_deltas(&current, &prior, one_hour);
        assert!((d.elapsed_days - one_hour).abs() < 1e-9);
        assert_eq!(d.changed.len(), 1);
    }

    /// The exchange-specific valuation groups join on their composite key, and the
    /// performance-percentage `sectors` group is excluded from diffing entirely.
    #[test]
    fn valuation_groups_join_and_sectors_are_excluded() {
        use crate::data_sources::SectorPerformance;
        let prior = BaselineMarketData {
            sector_pe: vec![SectorPe {
                sector: "Technology".into(),
                exchange: "NASDAQ".into(),
                pe: Some(30.0),
            }],
            market_risk_premium: vec![MarketRiskPremium {
                country: "United States".into(),
                country_risk_premium: 0.0,
                total_equity_risk_premium: 4.5,
            }],
            sectors: vec![SectorPerformance {
                sector: "Technology".into(),
                change_pct: 1.2,
            }],
            ..Default::default()
        };
        let current = BaselineMarketData {
            sector_pe: vec![SectorPe {
                sector: "Technology".into(),
                exchange: "NASDAQ".into(),
                pe: Some(31.5),
            }],
            market_risk_premium: vec![MarketRiskPremium {
                country: "United States".into(),
                country_risk_premium: 0.0,
                total_equity_risk_premium: 4.7,
            }],
            sectors: vec![SectorPerformance {
                sector: "Technology".into(),
                change_pct: -0.8,
            }],
            ..Default::default()
        };
        let d = compute_deltas(&current, &prior, 7.0);
        let pe = d
            .changed
            .iter()
            .find(|s| s.id == "Technology|NASDAQ")
            .unwrap();
        assert!((pe.abs_change - 1.5).abs() < 1e-9);
        let erp = d
            .changed
            .iter()
            .find(|s| s.group == GroupKind::MarketRiskPremium)
            .unwrap();
        assert!((erp.abs_change - 0.2).abs() < 1e-9);
        // `sectors` is a performance percentage — never diffed.
        assert!(!d.changed.iter().any(|s| s.group == GroupKind::Sectors));
    }

    /// A series that resolved last run but failed this run is `missing` *and* carries the
    /// gap reason from this run's manifest — so a transient feed outage isn't read as the
    /// series leaving the market.
    #[test]
    fn missing_series_carries_its_gap_reason_when_the_manifest_explains_it() {
        use crate::data_sources::DataGap;
        let prior = BaselineMarketData {
            internals: vec![
                quote("DTWEXBGS", "Dollar Index", 99.0),
                quote("DGS10", "10-Year Treasury Yield", 4.2),
            ],
            ..Default::default()
        };
        let current = BaselineMarketData {
            // DTWEXBGS failed to fetch this run: absent from the levels, recorded as a gap.
            internals: vec![quote("DGS10", "10-Year Treasury Yield", 4.25)],
            gaps: vec![DataGap::new(
                GroupKind::Internals,
                "DTWEXBGS",
                "Dollar Index",
                GapReason::Unavailable,
            )],
            ..Default::default()
        };
        let d = compute_deltas(&current, &prior, 7.0);
        let m = d.missing.iter().find(|t| t.id == "DTWEXBGS").unwrap();
        assert_eq!(m.reason, Some(GapReason::Unavailable));
    }

    /// A series absent with no gap recorded is a genuine coverage change — no reason.
    #[test]
    fn genuinely_absent_series_has_no_reason() {
        let prior = BaselineMarketData {
            internals: vec![quote("DGS2", "2-Year Treasury Yield", 4.0)],
            ..Default::default()
        };
        let current = BaselineMarketData::default();
        let d = compute_deltas(&current, &prior, 7.0);
        let m = d.missing.iter().find(|t| t.id == "DGS2").unwrap();
        assert_eq!(m.reason, None);
    }
}
