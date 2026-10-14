//! CFTC Commitments of Traders (COT) positioning adapter — the Step-3 baseline's
//! `cot_positioning` group (`docs/data-sources.md §CFTC`). Keyless, so it sits
//! outside the execution gate; every failure degrades to a recorded gap, and the
//! group carries no coverage floor.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use serde_json::{Map, Value};

use crate::cadence::ReportCadence;
use crate::data_sources::{
    emit_series_row, BaselineMarketData, CotPositioning, DataGap, GapReason, GroupKind,
    MarketDataSource,
};
use crate::progress::RunContext;

/// API origin for the CFTC public-reporting Socrata endpoints; dataset paths are joined
/// onto it per request.
const COT_BASE: &str = "https://publicreporting.cftc.gov";

/// Per-request timeout: the scan issues one request per tracked contract, none of which
/// should park for the model adapter's 120s ceiling.
const COT_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// Maximum acceptable staleness (today − the snapshot's report date), in days, before a
/// COT row is dropped rather than presented as current. The latest row is normally 3–11
/// days old (a Tuesday snapshot released the following Friday, more across a holiday), so
/// three weeks trips only on a genuinely stalled feed, never a normal week.
const MAX_STALENESS_DAYS: i64 = 21;

/// Which Socrata dataset (and thus trader-category schema) a contract reads from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CotReport {
    /// Traders in Financial Futures (`gpe5-46if`): dealer / asset-manager / leveraged-money
    /// split. The asset-manager ("real money") and leveraged-money ("fast money") lines are
    /// the signal.
    FinancialFutures,
    /// Disaggregated futures-only (`72hh-3qpy`): producer-merchant / managed-money split.
    /// Managed money is the speculator proxy; no asset-manager cohort exists, so the
    /// `real_money_*` lines stay `None`.
    Disaggregated,
}

impl CotReport {
    fn dataset(self) -> &'static str {
        match self {
            CotReport::FinancialFutures => "gpe5-46if",
            CotReport::Disaggregated => "72hh-3qpy",
        }
    }
}

/// One tracked contract, pinned by CFTC market code: names collide across micro /
/// consolidated variants, codes don't.
struct CotContract {
    code: &'static str,
    name: &'static str,
    asset_class: &'static str,
    report: CotReport,
}

/// The curated bellwether set tracked each run. Deliberately small: positioning extremes
/// on the index / rate / FX / metal / energy bellwethers are the signal, not breadth
/// across every contract.
const CONTRACTS: &[CotContract] = &[
    CotContract {
        code: "13874A",
        name: "E-Mini S&P 500",
        asset_class: "equity-index",
        report: CotReport::FinancialFutures,
    },
    CotContract {
        code: "209742",
        name: "Nasdaq-100 (Mini)",
        asset_class: "equity-index",
        report: CotReport::FinancialFutures,
    },
    CotContract {
        code: "043602",
        name: "10-Year U.S. Treasury Note",
        asset_class: "rates",
        report: CotReport::FinancialFutures,
    },
    CotContract {
        code: "042601",
        name: "2-Year U.S. Treasury Note",
        asset_class: "rates",
        report: CotReport::FinancialFutures,
    },
    CotContract {
        code: "098662",
        name: "U.S. Dollar Index",
        asset_class: "fx",
        report: CotReport::FinancialFutures,
    },
    CotContract {
        code: "088691",
        name: "Gold",
        asset_class: "commodity",
        report: CotReport::Disaggregated,
    },
    CotContract {
        code: "067651",
        name: "WTI Crude Oil",
        asset_class: "commodity",
        report: CotReport::Disaggregated,
    },
    CotContract {
        code: "085692",
        name: "Copper",
        asset_class: "commodity",
        report: CotReport::Disaggregated,
    },
];

/// One CFTC response classified into what the loop does with it — the single place the
/// degradation policy lives, in [`GapReason`] terms rather than a fatal `Err`.
enum Disposition {
    Row(Map<String, Value>),
    Gap(GapReason),
}

/// Interpret one Socrata response by status × body. An empty 2xx array is `Unavailable`
/// (no value this run, not a permanent absence); any 4xx other than 429 is `Rejected`,
/// fail-closed — a broken request degrades to a recorded gap, not a silent skip.
fn interpret_response(status: u16, body: &str) -> Disposition {
    match status {
        200..=299 => match serde_json::from_str::<Value>(body) {
            Ok(Value::Array(rows)) => match rows.into_iter().next() {
                Some(Value::Object(row)) => Disposition::Row(row),
                Some(_) => Disposition::Gap(GapReason::Malformed),
                None => Disposition::Gap(GapReason::Unavailable),
            },
            Ok(_) => Disposition::Gap(GapReason::Malformed),
            Err(_) => Disposition::Gap(GapReason::Malformed),
        },
        429 | 500..=599 => Disposition::Gap(GapReason::Unavailable),
        400..=499 => Disposition::Gap(GapReason::Rejected),
        _ => Disposition::Gap(GapReason::Malformed),
    }
}

/// Pull a numeric COT measure: Socrata serves every measure as a JSON *string*
/// (`"1199855"`), with an absent / empty / literal-`"null"` cell for a measure a report
/// doesn't carry. Parsed leniently to `Option<f64>`; a non-finite parse drops to `None`.
fn num(row: &Map<String, Value>, key: &str) -> Option<f64> {
    row.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("null"))
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite())
}

/// The Tuesday the snapshot reports, normalized from Socrata's floating-timestamp
/// (`"2026-06-16T00:00:00.000"`) to a plain `YYYY-MM-DD`. `None` when absent/empty.
fn report_date(row: &Map<String, Value>) -> Option<String> {
    let raw = row
        .get("report_date_as_yyyy_mm_dd")
        .and_then(Value::as_str)?
        .trim();
    if raw.is_empty() {
        return None;
    }
    Some(raw.split('T').next().unwrap_or(raw).to_string())
}

/// Whether a `report_date` (`YYYY-MM-DD`) is older than [`MAX_STALENESS_DAYS`] relative to
/// `today`. Fail-closed: an unparseable date can't be certified fresh, so it is stale too.
/// `today` is injected rather than read from the clock to keep this pure.
fn is_stale(report_date: &str, today: NaiveDate) -> bool {
    match NaiveDate::parse_from_str(report_date, "%Y-%m-%d") {
        Ok(date) => (today - date).num_days() > MAX_STALENESS_DAYS,
        Err(_) => true,
    }
}

/// Shape one Socrata row into a [`CotPositioning`]. The speculator line is leveraged
/// money (financial futures) or managed money (commodities); the asset-manager "real
/// money" line rides only on financial futures. Net is `long − short`. `None` when the
/// row's contract code is absent or mismatched (a fail-closed identity check), or when
/// it lacks a speculator leg, the open interest, or the report date — a gap, never a
/// fabricated zero.
fn row_to_positioning(row: &Map<String, Value>, contract: &CotContract) -> Option<CotPositioning> {
    // Fail-closed identity: a missing or mismatched code means the `$where` filter or
    // the dataset changed, so drop the row rather than stamp the requested identity onto
    // another contract's positioning.
    let code = row
        .get("cftc_contract_market_code")
        .and_then(Value::as_str)?;
    if code.trim() != contract.code {
        return None;
    }

    let report_date = report_date(row)?;
    let open_interest = num(row, "open_interest_all")?;

    // The trader-category field names differ by report. The TFF asset-manager/leveraged
    // fields carry no `_all` suffix; the disaggregated managed-money fields do — a real
    // inconsistency in the source schema, pinned here rather than guessed.
    let (long_k, short_k, chg_long_k, chg_short_k, pct_long_k) = match contract.report {
        CotReport::FinancialFutures => (
            "lev_money_positions_long",
            "lev_money_positions_short",
            "change_in_lev_money_long",
            "change_in_lev_money_short",
            "pct_of_oi_lev_money_long",
        ),
        CotReport::Disaggregated => (
            "m_money_positions_long_all",
            "m_money_positions_short_all",
            "change_in_m_money_long_all",
            "change_in_m_money_short_all",
            "pct_of_oi_m_money_long_all",
        ),
    };

    let spec_net = num(row, long_k)? - num(row, short_k)?;
    let spec_net_weekly_change = num(row, chg_long_k)
        .zip(num(row, chg_short_k))
        .map(|(l, s)| l - s);
    let spec_pct_oi_long = num(row, pct_long_k);

    // Asset-manager ("real money") line — financial futures only.
    let (real_money_net, real_money_net_weekly_change) = match contract.report {
        CotReport::FinancialFutures => (
            num(row, "asset_mgr_positions_long")
                .zip(num(row, "asset_mgr_positions_short"))
                .map(|(l, s)| l - s),
            num(row, "change_in_asset_mgr_long")
                .zip(num(row, "change_in_asset_mgr_short"))
                .map(|(l, s)| l - s),
        ),
        CotReport::Disaggregated => (None, None),
    };

    Some(CotPositioning {
        contract: contract.name.to_string(),
        contract_code: contract.code.to_string(),
        asset_class: contract.asset_class.to_string(),
        report_date,
        open_interest,
        spec_net,
        spec_net_weekly_change,
        spec_pct_oi_long,
        real_money_net,
        real_money_net_weekly_change,
    })
}

/// Live CFTC COT adapter behind the `MarketDataSource` trait; keyless, so outside the
/// execution gate.
pub struct CotDataSource {
    http: reqwest::blocking::Client,
    base_url: String,
    progress: Arc<RunContext>,
}

impl CotDataSource {
    pub fn new() -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(COT_TIMEOUT)
            .build()
            .context("building the CFTC COT HTTP client")?;
        Ok(Self {
            http,
            base_url: COT_BASE.to_string(),
            progress: RunContext::noop(),
        })
    }

    /// Point the adapter at a localhost mock so the wire path runs offline; a trailing
    /// slash is trimmed so the joined path's leading slash doesn't double up.
    #[cfg(test)]
    fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
    }

    /// Attach a live run context so the per-contract scan streams a tracker row per request
    /// and stops making requests once a cancel is observed.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// GET the latest weekly row for one contract, pinned by code and ordered newest-first,
    /// limit 1. A transport error returns `Err`, recorded by the caller as an `Unavailable`
    /// gap.
    fn get(&self, contract: &CotContract) -> Result<(u16, String)> {
        let url = format!(
            "{}/resource/{}.json",
            self.base_url,
            contract.report.dataset()
        );
        let where_clause = format!("cftc_contract_market_code='{}'", contract.code);
        crate::http_retry::send_with_retry("CFTC", || {
            self.http.get(&url).query(&[
                ("$where", where_clause.as_str()),
                ("$order", "report_date_as_yyyy_mm_dd DESC"),
                ("$limit", "1"),
            ])
        })
    }
}

impl CotDataSource {
    /// The scan loop, parameterised on `today` so the freshness guard stays pure (the trait
    /// method samples the real clock). Every failure — unreachable, rejected, malformed, or
    /// stale — degrades to a recorded gap.
    fn scan(&self, today: NaiveDate) -> Result<BaselineMarketData> {
        let mut cot_positioning = Vec::with_capacity(CONTRACTS.len());
        let mut gaps: Vec<DataGap> = Vec::new();
        for contract in CONTRACTS {
            if self.progress.is_cancelled() {
                break;
            }
            self.progress.request_started(
                "CFTC",
                GroupKind::CotPositioning.as_str(),
                contract.code,
                contract.name,
            );
            let gaps_before = gaps.len();
            let out_before = cot_positioning.len();
            let disposition = match self.get(contract) {
                Ok((status, body)) => interpret_response(status, &body),
                Err(_) => Disposition::Gap(GapReason::Unavailable), // transport — unreachable
            };
            match disposition {
                Disposition::Row(row) => match row_to_positioning(&row, contract) {
                    // A stale row (a stalled feed) degrades to Unavailable rather than
                    // passing as current.
                    Some(positioning) if !is_stale(&positioning.report_date, today) => {
                        cot_positioning.push(positioning)
                    }
                    Some(_) => gaps.push(DataGap::new(
                        GroupKind::CotPositioning,
                        contract.code,
                        contract.name,
                        GapReason::Unavailable,
                    )),
                    None => gaps.push(DataGap::new(
                        GroupKind::CotPositioning,
                        contract.code,
                        contract.name,
                        GapReason::Malformed,
                    )),
                },
                Disposition::Gap(reason) => gaps.push(DataGap::new(
                    GroupKind::CotPositioning,
                    contract.code,
                    contract.name,
                    reason,
                )),
            }
            emit_series_row(
                &self.progress,
                "CFTC",
                GroupKind::CotPositioning,
                contract.code,
                contract.name,
                &gaps,
                gaps_before,
                cot_positioning.len() > out_before,
                None,
            );
        }
        Ok(BaselineMarketData {
            cot_positioning,
            gaps,
            ..Default::default()
        })
    }
}

impl CotDataSource {
    /// The suite-facing positioning read (`docs/data-sources.md §CFTC`): the bellwether
    /// rows plus flattened gap notes, wholly fail-soft. `today` is the caller's pinned ET
    /// session, the freshness guard's anchor.
    pub fn positioning(
        &self,
        today: NaiveDate,
    ) -> (Vec<crate::data_sources::CotPositioning>, Vec<String>) {
        match self.scan(today) {
            Ok(data) => {
                let gaps = data
                    .gaps
                    .iter()
                    .map(|g| format!("{} ({}): {}", g.series_name, g.series_id, g.reason.as_str()))
                    .collect();
                (data.cot_positioning, gaps)
            }
            Err(e) => (Vec::new(), vec![format!("CFTC positioning unavailable: {e}")]),
        }
    }
}

impl MarketDataSource for CotDataSource {
    fn baseline_scan(&self, _cadence: ReportCadence) -> Result<BaselineMarketData> {
        // One clock sample anchors the freshness guard. Cadence is unused: COT carries its
        // own report date and week-over-week change, so it is exempt from the
        // report-over-report delta. The sample is the ET session date, not UTC: `is_stale`
        // compares it against the CFTC's own `report_date`, a market day, and a UTC date
        // would age an evening-ET run's report by one day.
        self.scan(crate::market_clock::et_session_date(Utc::now()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    /// A TFF (financial-futures) row: leveraged money net short, asset managers net long —
    /// the divergence the group exists to surface.
    const TFF_BODY: &str = r#"[{"cftc_contract_market_code":"13874A","report_date_as_yyyy_mm_dd":"2026-06-16T00:00:00.000","open_interest_all":"2579920","lev_money_positions_long":"157203","lev_money_positions_short":"672723","change_in_lev_money_long":"-11044","change_in_lev_money_short":"52890","pct_of_oi_lev_money_long":"6.1","asset_mgr_positions_long":"1199855","asset_mgr_positions_short":"215846","change_in_asset_mgr_long":"1000","change_in_asset_mgr_short":"2500"}]"#;

    /// A disaggregated (commodity) row: managed money net long, no asset-manager cohort.
    const DISAGG_BODY: &str = r#"[{"cftc_contract_market_code":"088691","report_date_as_yyyy_mm_dd":"2026-06-16T00:00:00.000","open_interest_all":"500000","m_money_positions_long_all":"250000","m_money_positions_short_all":"50000","change_in_m_money_long_all":"5000","change_in_m_money_short_all":"1000","pct_of_oi_m_money_long_all":"50.0"}]"#;

    /// A 200 TFF reply built per-contract so each carries its own `code` — the identity
    /// guard would drop a mismatched one. Leaked to satisfy the mock's `&'static str`.
    fn tff_reply(code: &str) -> Canned {
        let body = format!(
            r#"[{{"cftc_contract_market_code":"{code}","report_date_as_yyyy_mm_dd":"2026-06-16T00:00:00.000","open_interest_all":"2579920","lev_money_positions_long":"157203","lev_money_positions_short":"672723","change_in_lev_money_long":"-11044","change_in_lev_money_short":"52890","pct_of_oi_lev_money_long":"6.1","asset_mgr_positions_long":"1199855","asset_mgr_positions_short":"215846","change_in_asset_mgr_long":"1000","change_in_asset_mgr_short":"2500"}}]"#
        );
        Canned::Reply {
            status: 200,
            headers: vec![],
            body: Box::leak(body.into_boxed_str()),
        }
    }

    /// A 200 disaggregated (commodity) reply carrying `code`, like [`tff_reply`].
    fn disagg_reply(code: &str) -> Canned {
        let body = format!(
            r#"[{{"cftc_contract_market_code":"{code}","report_date_as_yyyy_mm_dd":"2026-06-16T00:00:00.000","open_interest_all":"500000","m_money_positions_long_all":"250000","m_money_positions_short_all":"50000","change_in_m_money_long_all":"5000","change_in_m_money_short_all":"1000","pct_of_oi_m_money_long_all":"50.0"}}]"#
        );
        Canned::Reply {
            status: 200,
            headers: vec![],
            body: Box::leak(body.into_boxed_str()),
        }
    }

    #[test]
    fn interpret_response_classifies_status_and_body() {
        assert!(matches!(
            interpret_response(200, TFF_BODY),
            Disposition::Row(_)
        ));
        assert!(matches!(
            interpret_response(200, "[]"),
            Disposition::Gap(GapReason::Unavailable)
        ));
        assert!(matches!(
            interpret_response(200, r#"{"error":"x"}"#),
            Disposition::Gap(GapReason::Malformed)
        ));
        assert!(matches!(
            interpret_response(200, "not json"),
            Disposition::Gap(GapReason::Malformed)
        ));
        assert!(matches!(
            interpret_response(429, ""),
            Disposition::Gap(GapReason::Unavailable)
        ));
        assert!(matches!(
            interpret_response(503, ""),
            Disposition::Gap(GapReason::Unavailable)
        ));
        assert!(matches!(
            interpret_response(400, "bad SoQL"),
            Disposition::Gap(GapReason::Rejected)
        ));
    }

    #[test]
    fn maps_a_financial_futures_row_with_both_cohorts() {
        let value: Value = serde_json::from_str(TFF_BODY).unwrap();
        let row = value.as_array().unwrap()[0].as_object().unwrap().clone();
        let contract = &CONTRACTS[0]; // E-Mini S&P 500, FinancialFutures
        let p = row_to_positioning(&row, contract).expect("maps");
        assert_eq!(p.contract, "E-Mini S&P 500");
        assert_eq!(p.report_date, "2026-06-16"); // floating-timestamp truncated
        assert_eq!(p.open_interest, 2_579_920.0);
        // Leveraged money net short: 157203 − 672723.
        assert_eq!(p.spec_net, -515_520.0);
        assert_eq!(p.spec_net_weekly_change, Some(-63_934.0)); // -11044 − 52890
        assert_eq!(p.spec_pct_oi_long, Some(6.1));
        // Asset managers net long: 1199855 − 215846.
        assert_eq!(p.real_money_net, Some(984_009.0));
        assert_eq!(p.real_money_net_weekly_change, Some(-1_500.0)); // 1000 − 2500
    }

    #[test]
    fn maps_a_commodity_row_without_a_real_money_cohort() {
        let value: Value = serde_json::from_str(DISAGG_BODY).unwrap();
        let row = value.as_array().unwrap()[0].as_object().unwrap().clone();
        let contract = &CONTRACTS[5]; // Gold, Disaggregated
        let p = row_to_positioning(&row, contract).expect("maps");
        assert_eq!(p.contract, "Gold");
        assert_eq!(p.asset_class, "commodity");
        assert_eq!(p.spec_net, 200_000.0); // managed money 250000 − 50000
        assert_eq!(p.spec_net_weekly_change, Some(4_000.0));
        assert_eq!(p.real_money_net, None); // no asset-manager cohort in disaggregated
        assert_eq!(p.real_money_net_weekly_change, None);
    }

    #[test]
    fn a_row_missing_a_net_leg_is_dropped_not_zeroed() {
        // The short leg is absent, so no honest net exists — a different failure than the
        // identity guard.
        let body = r#"{"cftc_contract_market_code":"13874A","report_date_as_yyyy_mm_dd":"2026-06-16T00:00:00.000","open_interest_all":"500000","lev_money_positions_long":"157203"}"#;
        let row: Map<String, Value> = serde_json::from_str(body).unwrap();
        assert!(row_to_positioning(&row, &CONTRACTS[0]).is_none());
    }

    #[test]
    fn contract_code_identity_is_validated() {
        let base: Value = serde_json::from_str(TFF_BODY).unwrap();
        let mut row = base.as_array().unwrap()[0].as_object().unwrap().clone();
        // CONTRACTS[0] is the S&P, 13874A.
        row.insert("cftc_contract_market_code".into(), Value::String("13874A".into()));
        assert!(row_to_positioning(&row, &CONTRACTS[0]).is_some());
        // A mismatched code must be dropped, not stamped with the requested identity — the
        // guard against a broken `$where` silently returning another contract's data.
        row.insert("cftc_contract_market_code".into(), Value::String("999999".into()));
        assert!(row_to_positioning(&row, &CONTRACTS[0]).is_none());
        row.remove("cftc_contract_market_code");
        assert!(row_to_positioning(&row, &CONTRACTS[0]).is_none());
    }

    #[test]
    fn freshness_guard_flags_a_stalled_snapshot() {
        let today = NaiveDate::from_ymd_opt(2026, 6, 23).unwrap();
        assert!(!is_stale("2026-06-16", today), "last week's snapshot is fresh");
        assert!(!is_stale("2026-06-02", today), "21 days is just inside the bound");
        assert!(is_stale("2026-06-01", today), "22 days is a stalled feed");
        assert!(
            is_stale("not-a-date", today),
            "an uncertifiable date is treated as stale"
        );
    }

    #[test]
    fn baseline_scan_round_trips_every_contract_offline() {
        // One canned reply per tracked contract, in the order the scan walks `CONTRACTS`,
        // each carrying that contract's own code so the identity guard passes.
        let script: Vec<Canned> = CONTRACTS
            .iter()
            .map(|c| match c.report {
                CotReport::FinancialFutures => tff_reply(c.code),
                CotReport::Disaggregated => disagg_reply(c.code),
            })
            .collect();
        let server = MockHttp::serve(script);
        let source = CotDataSource::new().unwrap().with_base_url(&server.base_url);
        // A fixed "today" a few days after the fixtures' snapshot, so the freshness guard
        // passes deterministically.
        let today = NaiveDate::from_ymd_opt(2026, 6, 20).unwrap();
        let data = source.scan(today).unwrap();

        assert_eq!(data.cot_positioning.len(), CONTRACTS.len());
        assert!(data.gaps.is_empty());
        assert_eq!(server.attempts(), CONTRACTS.len());

        let spx = &data.cot_positioning[0];
        assert_eq!(spx.contract, "E-Mini S&P 500");
        assert_eq!(spx.spec_net, -515_520.0);
        assert_eq!(spx.real_money_net, Some(984_009.0));
        let gold = &data.cot_positioning[5];
        assert_eq!(gold.contract, "Gold");
        assert_eq!(gold.real_money_net, None);

        let paths = server.request_paths();
        assert_eq!(paths[0], "/resource/gpe5-46if.json");
        assert_eq!(paths[5], "/resource/72hh-3qpy.json");
        // request_paths strips the query, so assert the full target to catch an unpinned
        // `$where` filter.
        let targets = server.request_targets();
        assert!(
            targets[0].contains("13874A"),
            "S&P request must filter by its code: {}",
            targets[0]
        );
        assert!(
            targets[5].contains("088691"),
            "Gold request must filter by its code: {}",
            targets[5]
        );
    }

    #[test]
    #[ignore = "hits the live CFTC COT API (keyless)"]
    fn cot_baseline_smoke() {
        let src = CotDataSource::new().expect("build COT source");
        let data = src
            .baseline_scan(ReportCadence::default())
            .expect("live COT scan");

        // Dump the resolved positioning so a maintainer can eyeball it (run with
        // `-- --ignored --nocapture`); this is where a renamed / delisted contract code or
        // a changed trader-category field name surfaces.
        eprintln!("cot_positioning ({}):", data.cot_positioning.len());
        for p in &data.cot_positioning {
            eprintln!(
                "  {:<28} code={:<8} {} OI={:>11} spec_net={:>11} real_money_net={:?}",
                p.contract,
                p.contract_code,
                p.report_date,
                p.open_interest,
                p.spec_net,
                p.real_money_net
            );
        }
        for g in &data.gaps {
            eprintln!("  GAP {} {} — {}", g.series_id, g.series_name, g.reason.as_str());
        }

        // Every tracked contract must resolve — a renamed code or a changed field name
        // would silently thin the group.
        assert_eq!(
            data.cot_positioning.len(),
            CONTRACTS.len(),
            "a tracked COT contract did not resolve"
        );
        assert!(
            data.gaps.is_empty(),
            "live COT scan recorded gaps: {:?}",
            data.gaps
        );
    }
}
