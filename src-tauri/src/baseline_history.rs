//! Bounded recent history of the baseline's level-bearing series: the retained snapshots
//! (`storage::baseline_snapshots`, `docs/storage.md §Baseline Snapshots`) joined by series
//! id into one aligned table, one point per retained report plus this run, oldest first.
//! The series set is the change view's own [`DELTA_GROUPS`]. Additive context: the history
//! is `None` below [`MIN_HISTORY_POINTS`] and never gates a run.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::baseline_delta::{series_levels, DELTA_GROUPS};
use crate::data_sources::{BaselineMarketData, GroupKind};
use crate::market_clock;

/// The fewest points the history renders at. Two points — one prior report and this
/// run — would only restate the change view's prior and current levels.
pub const MIN_HISTORY_POINTS: usize = 3;

/// One point of the history: a retained report's snapshot, or this run.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryPoint {
    /// The snapshot's capture instant as its ET date and time (`market_clock::et_stamp`).
    pub at: String,
    /// The report the snapshot backs, the id its summary metadata carries. `None` for
    /// this run, whose report does not exist yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
}

/// One series across the history's points, joined by id.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeriesHistory {
    pub group: GroupKind,
    pub id: String,
    /// The name from the newest point carrying the series.
    pub name: String,
    /// One level per point, aligned with [`BaselineHistory::points`]; `None` (a JSON
    /// `null`) where that report's scan did not gather the series.
    pub values: Vec<Option<f64>>,
}

/// The rendered history: the points oldest first with this run last, and every series
/// any point carried.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BaselineHistory {
    pub points: Vec<HistoryPoint>,
    pub series: Vec<SeriesHistory>,
}

/// One retained snapshot, decoded: the report it backs, its capture instant, and its scan.
#[derive(Debug, Clone)]
pub struct PriorBaseline {
    pub report_id: String,
    pub captured_at: DateTime<Utc>,
    pub baseline: BaselineMarketData,
}

/// Build the history from this run's scan and the retained snapshots, `priors` newest
/// first (the storage read's order). `None` when the points fall below
/// [`MIN_HISTORY_POINTS`] or no point carries a level-bearing series.
///
/// Series are ordered by [`DELTA_GROUPS`]; within a group, this run's series come first in
/// its order, then any series only a prior report carried, newest prior first. A
/// non-finite level reads as absent.
pub fn build_history(
    current: &BaselineMarketData,
    as_of: DateTime<Utc>,
    priors: &[PriorBaseline],
) -> Option<BaselineHistory> {
    if priors.len() + 1 < MIN_HISTORY_POINTS {
        return None;
    }

    // Newest first, like `priors`: this run, then each prior. Reversed for the points.
    let scans: Vec<&BaselineMarketData> = std::iter::once(current)
        .chain(priors.iter().map(|p| &p.baseline))
        .collect();

    let mut points: Vec<HistoryPoint> = priors
        .iter()
        .rev()
        .map(|p| HistoryPoint {
            at: market_clock::et_stamp(p.captured_at),
            report_id: Some(p.report_id.clone()),
        })
        .collect();
    points.push(HistoryPoint {
        at: market_clock::et_stamp(as_of),
        report_id: None,
    });

    let mut series = Vec::new();
    for group in DELTA_GROUPS {
        let per_scan: Vec<Vec<(String, String, f64)>> = scans
            .iter()
            .map(|scan| series_levels(scan, group))
            .collect();
        let by_id: Vec<HashMap<&str, f64>> = per_scan
            .iter()
            .map(|levels| {
                levels
                    .iter()
                    .filter(|(_, _, level)| level.is_finite())
                    .map(|(id, _, level)| (id.as_str(), *level))
                    .collect()
            })
            .collect();

        // First sighting newest-first fixes both the row order and the newest name.
        let mut seen: HashSet<&str> = HashSet::new();
        for levels in &per_scan {
            for (id, name, level) in levels {
                if !level.is_finite() || !seen.insert(id.as_str()) {
                    continue;
                }
                let values = by_id
                    .iter()
                    .rev()
                    .map(|scan| scan.get(id.as_str()).copied())
                    .collect();
                series.push(SeriesHistory {
                    group,
                    id: id.clone(),
                    name: name.clone(),
                    values,
                });
            }
        }
    }

    if series.is_empty() {
        return None;
    }
    Some(BaselineHistory { points, series })
}

/// The history as the prompt block both the main agent and the analysts read: the
/// framing sentence, the points header, and one compact JSON line per series.
pub fn render_block(history: &BaselineHistory) -> String {
    let mut block = String::from(
        "Baseline history — each level-bearing series at every retained prior report and \
         this run, oldest first, this run last. Points are one per report, unevenly spaced \
         in time, each dated in ET; a null is a series not gathered at that report.",
    );
    block.push_str("\npoints: ");
    block.push_str(&serde_json::to_string(&history.points).unwrap_or_else(|_| "[]".into()));
    for row in &history.series {
        if let Ok(line) = serde_json::to_string(row) {
            block.push('\n');
            block.push_str(&line);
        }
    }
    block
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_sources::{Change, MarketRiskPremium, Quote, SectorPe};
    use chrono::TimeZone;

    fn quote(symbol: &str, name: &str, price: f64) -> Quote {
        Quote {
            symbol: symbol.into(),
            name: name.into(),
            price,
            change: Change::percent(0.0),
            unit: "percent".into(),
        }
    }

    fn scan(internals: Vec<Quote>) -> BaselineMarketData {
        BaselineMarketData {
            internals,
            ..Default::default()
        }
    }

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, day, hour, 0, 0).unwrap()
    }

    fn prior(
        report_id: &str,
        captured_at: DateTime<Utc>,
        baseline: BaselineMarketData,
    ) -> PriorBaseline {
        PriorBaseline {
            report_id: report_id.into(),
            captured_at,
            baseline,
        }
    }

    #[test]
    fn points_run_oldest_first_with_this_run_last_and_values_align() {
        let ten = |level: f64| scan(vec![quote("DGS10", "10-Year Treasury Yield", level)]);
        let current = ten(4.33);
        // Newest first, as the storage read returns them.
        let priors = [
            prior("rep-b", at(22, 20), ten(4.15)),
            prior("rep-a", at(15, 20), ten(4.10)),
        ];
        let history = build_history(&current, at(29, 19), &priors).unwrap();

        assert_eq!(
            history.points,
            [
                HistoryPoint {
                    at: "2026-09-15 16:00 ET".into(),
                    report_id: Some("rep-a".into()),
                },
                HistoryPoint {
                    at: "2026-09-22 16:00 ET".into(),
                    report_id: Some("rep-b".into()),
                },
                HistoryPoint {
                    at: "2026-09-29 15:00 ET".into(),
                    report_id: None,
                },
            ]
        );
        assert_eq!(history.series.len(), 1);
        let row = &history.series[0];
        assert_eq!(
            (row.group, row.id.as_str()),
            (GroupKind::Internals, "DGS10")
        );
        assert_eq!(row.values, [Some(4.10), Some(4.15), Some(4.33)]);
    }

    #[test]
    fn a_series_absent_from_a_point_reads_null_and_prior_only_series_are_kept() {
        let current = scan(vec![
            quote("DGS2", "2-Year Treasury Yield", 3.70),
            quote("DGS10", "10-Year Treasury Yield", 4.33),
        ]);
        let priors = [
            // DGS2 not gathered here (a gap); T10Y2Y still served.
            prior(
                "rep-b",
                at(22, 20),
                scan(vec![
                    quote("DGS10", "10-Year Treasury Yield", 4.15),
                    quote("T10Y2Y", "10-Year minus 2-Year Treasury Spread", 0.55),
                ]),
            ),
            prior(
                "rep-a",
                at(15, 20),
                scan(vec![
                    quote("DGS2", "2-Year Treasury Yield", 3.61),
                    quote("DGS10", "10-Year Treasury Yield (old name)", 4.10),
                    quote("T10Y2Y", "10-Year minus 2-Year Treasury Spread", 0.49),
                ]),
            ),
        ];
        let history = build_history(&current, at(29, 19), &priors).unwrap();

        let ids: Vec<&str> = history.series.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["DGS2", "DGS10", "T10Y2Y"], "this run's order, then prior-only");
        assert_eq!(history.series[0].values, [Some(3.61), None, Some(3.70)]);
        assert_eq!(
            history.series[1].name, "10-Year Treasury Yield",
            "the name comes from the newest point carrying the series"
        );
        assert_eq!(history.series[2].values, [Some(0.49), Some(0.55), None]);
    }

    #[test]
    fn groups_follow_the_change_views_level_groups_and_a_band_dropped_pe_reads_null() {
        let sector = |pe: Option<f64>| SectorPe {
            sector: "Energy".into(),
            exchange: "NYSE".into(),
            pe,
            pe_history: None,
            trailing_return: None,
        };
        let with = |pe: Option<f64>, vix: f64| BaselineMarketData {
            indices: vec![quote("^GSPC", "S&P 500", 6_000.0)],
            internals: vec![quote("^VIX", "CBOE Volatility Index", vix)],
            sector_pe: vec![sector(pe)],
            market_risk_premium: vec![MarketRiskPremium {
                country: "United States".into(),
                country_risk_premium: 0.0,
                total_equity_risk_premium: 4.6,
            }],
            ..Default::default()
        };
        let priors = [
            prior("rep-b", at(22, 20), with(None, 15.0)),
            prior("rep-a", at(15, 20), with(Some(12.0), 14.0)),
        ];
        let history = build_history(&with(Some(13.0), 16.0), at(29, 19), &priors).unwrap();

        let groups: Vec<GroupKind> = history.series.iter().map(|s| s.group).collect();
        assert_eq!(
            groups,
            [
                GroupKind::Indices,
                GroupKind::Internals,
                GroupKind::SectorPe,
                GroupKind::MarketRiskPremium
            ]
        );
        let pe = &history.series[2];
        assert_eq!(pe.id, "Energy|NYSE");
        assert_eq!(pe.values, [Some(12.0), None, Some(13.0)]);
    }

    #[test]
    fn fewer_than_three_points_or_no_level_series_yield_no_history() {
        let current = scan(vec![quote("DGS10", "10-Year Treasury Yield", 4.33)]);
        let one = [prior("rep-a", at(15, 20), current.clone())];
        assert!(build_history(&current, at(29, 19), &[]).is_none(), "first report");
        assert!(
            build_history(&current, at(29, 19), &one).is_none(),
            "two points restate the change view"
        );

        let empty = BaselineMarketData::default();
        let priors = [
            prior("rep-b", at(22, 20), empty.clone()),
            prior("rep-a", at(15, 20), empty.clone()),
        ];
        assert!(build_history(&empty, at(29, 19), &priors).is_none());
    }

    #[test]
    fn the_block_frames_the_points_and_puts_one_series_on_each_line() {
        let ten = |level: f64| scan(vec![quote("DGS10", "10-Year Treasury Yield", level)]);
        let priors = [
            prior("rep-b", at(22, 20), scan(vec![])),
            prior("rep-a", at(15, 20), ten(4.1)),
        ];
        let block = render_block(&build_history(&ten(4.33), at(29, 19), &priors).unwrap());
        assert_eq!(
            block,
            "Baseline history — each level-bearing series at every retained prior report and \
             this run, oldest first, this run last. Points are one per report, unevenly spaced \
             in time, each dated in ET; a null is a series not gathered at that report.\n\
             points: [{\"at\":\"2026-09-15 16:00 ET\",\"report_id\":\"rep-a\"},\
             {\"at\":\"2026-09-22 16:00 ET\",\"report_id\":\"rep-b\"},\
             {\"at\":\"2026-09-29 15:00 ET\"}]\n\
             {\"group\":\"internals\",\"id\":\"DGS10\",\"name\":\"10-Year Treasury Yield\",\
             \"values\":[4.1,null,4.33]}"
        );
    }
}
