//! The per-holding **evidence surface** of the FMP endpoint table
//! (`docs/data-sources.md` §Portfolio Analysis — endpoint surface) — the rows
//! FETCHED VALUES renders as the providers return them and the engine never
//! reads (`docs/portfolio-workflow.md` §Step 6c): the eight TTM ratio lines,
//! owner earnings, enterprise value, the street price-target consensus with
//! its trend, the analyst grades consensus with the rating actions, FMP's
//! ratings snapshot, the insider and congressional trades, the float, the
//! revenue segments, the splits feed and the earnings-surprise history. The
//! adapter fills one record per graded stock
//! ([`crate::fmp::FmpDataSource::fetch_company_evidence`]), every leg
//! fail-soft to a tagged gap on the record; the dossier carries it
//! (`HoldingDossier::evidence`) and the gaps join the financials' gap
//! manifest at the gather, so they reach the audit and data health the way
//! the SEC and deep-history gaps do. [`CompanyFinancials`] stays the engine's
//! input struct; nothing here feeds a sub-score, a band or a rung.
//!
//! The counts and windows are the plan-time drafts (ruled 2026-10-08): rating
//! actions the latest ten within twelve months; insider trades the latest ten
//! within six months plus the statistics line; congressional trades the
//! latest ten, Senate and House merged, within twelve months; the surprise
//! history eight quarters; the segments the latest two fiscal years, every
//! segment; the M&A feed the trailing twelve months under a ten-page walk.
//!
//! [`CompanyFinancials`]: crate::portfolio::engine::CompanyFinancials

use serde::{Deserialize, Serialize};

/// Rating actions rendered — the latest this many inside the window.
pub const RATING_ACTIONS_COUNT: usize = 10;
/// Rating actions window, in months before the session date.
pub const RATING_ACTIONS_WINDOW_MONTHS: u32 = 12;
/// Insider trades rendered — the latest this many inside the window.
pub const INSIDER_TRADES_COUNT: usize = 10;
/// Insider trades window, in months before the session date.
pub const INSIDER_TRADES_WINDOW_MONTHS: u32 = 6;
/// Congressional trades rendered, Senate and House merged — the latest this
/// many inside the window.
pub const CONGRESSIONAL_TRADES_COUNT: usize = 10;
/// Congressional trades window, in months before the session date.
pub const CONGRESSIONAL_TRADES_WINDOW_MONTHS: u32 = 12;
/// Earnings-surprise rows rendered — the latest this many reported quarters.
pub const SURPRISE_QUARTERS: usize = 8;
/// Revenue segment years rendered — the latest this many fiscal years.
pub const SEGMENT_FISCAL_YEARS: usize = 2;
/// Dividend payments rendered — the latest this many on or before the session.
pub const DIVIDENDS_SHOWN: usize = 4;
/// A fund's country weightings rendered — the largest this many (every
/// sector weighting renders).
pub const FUND_COUNTRY_WEIGHTS_SHOWN: usize = 10;
/// The M&A feed window, in months before the session date.
pub const MA_WINDOW_MONTHS: u32 = 12;
/// The M&A feed's page walk cap (pages of [`MA_PAGE_LIMIT`] rows, newest
/// first) — the report-froth item's drafted budget.
pub const MA_PAGE_CAP: usize = 10;
/// Rows per M&A feed page — a page served short of it ends the walk.
pub const MA_PAGE_LIMIT: usize = 100;

/// The eight TTM ratio lines (ruled 2026-10-08): P/E and P/B off `ratios-ttm`,
/// EV/EBITDA, EV/sales, FCF yield, ROIC, ROE and net debt/EBITDA off
/// `key-metrics-ttm`. Each `None` where its pull gapped or the field was
/// absent; the pulls' gaps ride the record.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RatioLines {
    pub pe: Option<f64>,
    pub pb: Option<f64>,
    pub ev_to_ebitda: Option<f64>,
    pub ev_to_sales: Option<f64>,
    pub fcf_yield: Option<f64>,
    pub roic: Option<f64>,
    pub roe: Option<f64>,
    pub net_debt_to_ebitda: Option<f64>,
}

impl RatioLines {
    /// No line served at all.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The latest `owner-earnings` row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OwnerEarningsRow {
    /// The period end, ISO.
    pub period_end: String,
    /// The fiscal period label as served (`FY2025 Q1`), where readable.
    pub period: Option<String>,
    pub owners_earnings: Option<f64>,
    pub per_share: Option<f64>,
}

/// The latest `enterprise-values` row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EnterpriseValueRow {
    pub date: String,
    pub enterprise_value: Option<f64>,
    pub market_cap: Option<f64>,
    pub total_debt: Option<f64>,
    pub cash: Option<f64>,
}

/// `price-target-consensus` — the street's level.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PriceTargetConsensus {
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub median: Option<f64>,
    pub consensus: Option<f64>,
}

/// One window of `price-target-summary`: how many targets were published and
/// their average.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PriceTargetWindow {
    pub count: Option<u64>,
    pub average: Option<f64>,
}

/// `price-target-summary` — the street's trend over the month, quarter and
/// year.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PriceTargetTrend {
    pub last_month: PriceTargetWindow,
    pub last_quarter: PriceTargetWindow,
    pub last_year: PriceTargetWindow,
}

/// `grades-consensus` — the analyst buy / hold / sell counts and the word.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GradesConsensus {
    pub strong_buy: Option<u64>,
    pub buy: Option<u64>,
    pub hold: Option<u64>,
    pub sell: Option<u64>,
    pub strong_sell: Option<u64>,
    pub consensus: Option<String>,
}

/// One `grades` row — a rating action.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RatingAction {
    /// The action's date, ISO.
    pub date: String,
    pub firm: String,
    pub previous_grade: Option<String>,
    pub new_grade: Option<String>,
    /// `upgrade`, `downgrade`, `maintain`, `initialise`, … as served.
    pub action: Option<String>,
}

/// `ratings-snapshot` — FMP's own composite rating and its component scores.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RatingsSnapshot {
    pub rating: Option<String>,
    pub overall: Option<i64>,
    pub discounted_cash_flow: Option<i64>,
    pub return_on_equity: Option<i64>,
    pub return_on_assets: Option<i64>,
    pub debt_to_equity: Option<i64>,
    pub price_to_earnings: Option<i64>,
    pub price_to_book: Option<i64>,
}

/// One `insider-trading/search` row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InsiderTrade {
    /// The transaction date, ISO — the window's key.
    pub transaction_date: String,
    pub filing_date: Option<String>,
    pub name: String,
    /// `director`, `officer`, `10 percent owner`, … as served.
    pub owner_type: Option<String>,
    /// `P-Purchase`, `S-Sale`, `A-Award`, … as served.
    pub transaction_type: Option<String>,
    pub shares: Option<f64>,
    pub price: Option<f64>,
}

/// `insider-trading/statistics` — the latest quarter's line.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InsiderStatistics {
    pub year: Option<i64>,
    pub quarter: Option<i64>,
    pub acquired_transactions: Option<u64>,
    pub disposed_transactions: Option<u64>,
    pub total_acquired: Option<f64>,
    pub total_disposed: Option<f64>,
}

/// Which chamber a congressional trade was disclosed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chamber {
    Senate,
    House,
}

impl Chamber {
    pub fn label(self) -> &'static str {
        match self {
            Chamber::Senate => "Senate",
            Chamber::House => "House",
        }
    }
}

/// One `senate-trades` / `house-trades` row.
#[derive(Debug, Clone, PartialEq)]
pub struct CongressionalTrade {
    pub chamber: Chamber,
    /// The transaction date, ISO — the window's key.
    pub transaction_date: String,
    pub disclosure_date: Option<String>,
    pub name: String,
    /// `Self`, `Spouse`, `Joint`, … as served.
    pub owner: Option<String>,
    /// `Purchase`, `Sale`, … as served.
    pub kind: Option<String>,
    /// The disclosed amount band as served (`$1,001 - $15,000`).
    pub amount: Option<String>,
}

/// `shares-float` — the float row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SharesFloat {
    /// The row's date as served (the feed stamps a timestamp).
    pub date: Option<String>,
    /// The free float as a percentage of shares outstanding, as served.
    pub free_float_percent: Option<f64>,
    pub float_shares: Option<f64>,
    pub outstanding_shares: Option<f64>,
}

/// One fiscal year of a revenue segmentation feed — the segments as reported,
/// largest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SegmentYear {
    pub fiscal_year: Option<i64>,
    /// The period end, ISO.
    pub period_end: String,
    pub segments: Vec<(String, f64)>,
}

/// One `splits` row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SplitRow {
    /// The split date, ISO.
    pub date: String,
    pub numerator: f64,
    pub denominator: f64,
}

/// One dividend payment off the `dividends` feed — the ex-date and the amount
/// as the feed reports them, with the payment date where served.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DividendRow {
    /// The ex-dividend date, ISO.
    pub date: String,
    pub amount: f64,
    pub payment_date: Option<String>,
}

/// One row of the run-level `mergers-acquisitions-latest` feed — persisted on
/// the checkpoint header beside the FINRA file so a resumed run reads the same
/// feed (`docs/portfolio-analysis.md` §Failure posture).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MaDeal {
    /// The acquirer's symbol as served (may carry a share-class suffix).
    pub acquirer_symbol: Option<String>,
    pub acquirer_name: Option<String>,
    pub target_symbol: Option<String>,
    pub target_name: Option<String>,
    /// The transaction date, ISO — the window's key.
    pub transaction_date: String,
    pub link: Option<String>,
}

/// Which side of a deal the holding is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaRole {
    Acquirer,
    Target,
}

/// A holding's match against the run-level M&A feed
/// (`dossier::ma_matches_for_holding`).
#[derive(Debug, Clone, PartialEq)]
pub struct MaMatch {
    pub role: MaRole,
    /// The other party's name (its symbol where the feed served no name).
    pub counterparty: String,
    pub date: String,
    pub link: Option<String>,
}

/// The per-holding evidence record ([module docs](self)).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompanyEvidence {
    pub symbol: String,
    pub ratios: RatioLines,
    pub owner_earnings: Option<OwnerEarningsRow>,
    pub enterprise_value: Option<EnterpriseValueRow>,
    pub price_target: Option<PriceTargetConsensus>,
    pub price_target_trend: Option<PriceTargetTrend>,
    pub grades_consensus: Option<GradesConsensus>,
    /// Newest first, inside the window, at most [`RATING_ACTIONS_COUNT`].
    pub rating_actions: Vec<RatingAction>,
    pub ratings_snapshot: Option<RatingsSnapshot>,
    /// Newest first, inside the window, at most [`INSIDER_TRADES_COUNT`].
    pub insider_trades: Vec<InsiderTrade>,
    pub insider_statistics: Option<InsiderStatistics>,
    /// Newest first, both chambers merged, inside the window, at most
    /// [`CONGRESSIONAL_TRADES_COUNT`].
    pub congressional_trades: Vec<CongressionalTrade>,
    pub float: Option<SharesFloat>,
    /// Newest first, at most [`SEGMENT_FISCAL_YEARS`].
    pub product_segments: Vec<SegmentYear>,
    pub geographic_segments: Vec<SegmentYear>,
    /// Newest first — the split-context line's date and ratio source.
    pub splits: Vec<SplitRow>,
    /// Newest first — the reported rows (an actual landed), at most
    /// [`SURPRISE_QUARTERS`], plus the next announcement where the feed carries
    /// it ([`Self::next_earnings`]).
    pub earnings: Vec<crate::fmp::SymbolEarningsRow>,
    /// Tagged legs that gapped — joined onto the financials' gap manifest at the
    /// gather.
    pub gaps: Vec<String>,
}

impl CompanyEvidence {
    /// An unfilled record for `symbol` — the offline default (no gap: a stub
    /// that never wired the leg is not a degraded live input).
    pub fn empty(symbol: &str) -> Self {
        Self {
            symbol: symbol.to_string(),
            ..Default::default()
        }
    }

    /// The reported surprise rows (an EPS actual landed), newest first.
    pub fn reported_earnings(&self) -> impl Iterator<Item = &crate::fmp::SymbolEarningsRow> {
        self.earnings.iter().filter(|r| r.eps_actual.is_some())
    }

    /// The next announcement the feed carries — the nearest row with no actual
    /// dated on or after `session` (ISO), so the announcement day's own row
    /// is "next" until its actual lands (Codex, 2026-10-08) — if any.
    pub fn next_earnings(&self, session: &str) -> Option<&crate::fmp::SymbolEarningsRow> {
        self.earnings
            .iter()
            .filter(|r| r.eps_actual.is_none() && r.date.as_str() >= session)
            .min_by(|a, b| a.date.cmp(&b.date))
    }
}

/// The ISO date `months` months before `session`, for the window filters.
pub fn window_start(session: chrono::NaiveDate, months: u32) -> String {
    session
        .checked_sub_months(chrono::Months::new(months))
        .unwrap_or(session)
        .format("%Y-%m-%d")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_start_is_the_session_less_the_months() {
        let session = chrono::NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        assert_eq!(window_start(session, 12), "2025-10-08");
        assert_eq!(window_start(session, 6), "2026-04-08");
    }

    #[test]
    fn reported_rows_and_the_next_announcement_split_on_the_actual() {
        let row = |date: &str, actual: Option<f64>| crate::fmp::SymbolEarningsRow {
            date: date.into(),
            eps_actual: actual,
            eps_estimated: Some(1.0),
            revenue_actual: None,
        };
        let e = CompanyEvidence {
            earnings: vec![
                row("2026-10-21", None),
                row("2026-07-22", Some(1.1)),
                row("2026-04-22", Some(0.9)),
            ],
            ..CompanyEvidence::empty("TSLA")
        };
        assert_eq!(e.reported_earnings().count(), 2);
        assert_eq!(
            e.next_earnings("2026-10-08").map(|r| r.date.as_str()),
            Some("2026-10-21")
        );
        // The announcement day's own row is next until its actual lands.
        assert_eq!(e.next_earnings("2026-10-21").map(|r| r.date.as_str()), Some("2026-10-21"));
        // A past row with no actual is a gap in the feed, never "next".
        assert!(e.next_earnings("2026-10-22").is_none());
        assert!(RatioLines::default().is_empty());
    }
}
