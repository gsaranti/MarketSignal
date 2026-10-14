//! The live Schwab Trader API holdings source (`docs/schwab-integration.md §What is
//! pulled`), behind the same [`HoldingsSource`] as the fixture.
//!
//! Read-only by construction: the Trader API has no read-only token scope, so the
//! guarantee lives here. The adapter builds only `GET`s and exposes no order path, and
//! the GET-only test pins it.
//!
//! Access tokens come from a [`TokenProvider`] closure, so the refresh lifecycle stays
//! in `schwab_oauth`. The token rides an `Authorization` header and never reaches a
//! log line or the run tracker.

use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{Duration, NaiveDate, Utc};
use serde_json::Value;

use crate::http_retry::send_with_retry;
use crate::portfolio::AssetClass;
use crate::schwab::{Holdings, HoldingsSource, OptionChain, OptionKind, OptionQuote, Position};

/// Schwab's API host — both the Trader (`/trader/v1`) and Market Data
/// (`/marketdata/v1`) products live under it.
const SCHWAB_API_BASE: &str = "https://api.schwabapi.com";

/// Strikes each side of at-the-money to request, and how many days of expirations to
/// span: bounds the `/chains` payload so a heavily-optioned name can't return a
/// multi-thousand-contract response (`docs/schwab-integration.md §What is pulled`).
const CHAIN_STRIKE_COUNT: u32 = 12;
const CHAIN_WINDOW_DAYS: i64 = 60;

/// Supplies a currently-valid access token for one API call; in production it closes
/// over the OAuth client's `valid_access_token`.
pub type TokenProvider = Arc<dyn Fn() -> Result<String> + Send + Sync>;

/// `base` is a field so tests can point the calls at a localhost mock.
pub struct SchwabApiSource {
    http: reqwest::blocking::Client,
    base: String,
    token: TokenProvider,
}

impl SchwabApiSource {
    /// Build against Schwab's real API host.
    pub fn new(token: TokenProvider) -> Result<Self> {
        Ok(Self {
            http: reqwest::blocking::Client::builder()
                .build()
                .context("building Schwab API HTTP client")?,
            base: SCHWAB_API_BASE.to_string(),
            token,
        })
    }

    /// Test seam: point the calls at a localhost mock and hand in a static token.
    #[cfg(test)]
    pub fn with_base_url(base: impl Into<String>, token: TokenProvider) -> Self {
        Self {
            http: reqwest::blocking::Client::new(),
            base: base.into(),
            token,
        }
    }

    /// The token rides an `Authorization` header, never the URL or the error context.
    fn get(&self, url: &str, token: &str, label: &str) -> Result<(u16, String)> {
        send_with_retry(label, || self.http.get(url).bearer_auth(token))
    }
}

impl HoldingsSource for SchwabApiSource {
    fn holdings(&self) -> Result<Holdings> {
        let token = (self.token)()?;

        // Schwab identifies accounts by an opaque hash, not the plaintext number.
        let (status, body) = self.get(
            &format!("{}/trader/v1/accounts/accountNumbers", self.base),
            &token,
            "schwab account list",
        )?;
        if status != 200 {
            bail!("Schwab account-list request failed (HTTP {status})");
        }
        let hashes = parse_account_hashes(&body)?;
        if hashes.is_empty() {
            bail!("Schwab returned no accounts for this login");
        }

        // Each account is reconciled against Schwab's liquidation value before its
        // rows join the book, so a bank sweep reported both as CASH_EQUIVALENT and
        // in cashBalance is counted once rather than inflating every weight.
        let mut positions: Vec<Position> = Vec::new();
        let mut source_rows: Vec<Position> = Vec::new();
        let mut cash = 0.0;
        let mut account_total = 0.0;
        for hash in hashes {
            let (status, body) = self.get(
                &format!("{}/trader/v1/accounts/{hash}?fields=positions", self.base),
                &token,
                "schwab positions",
            )?;
            if status != 200 {
                bail!("Schwab positions request failed (HTTP {status})");
            }
            let account = reconcile_account(parse_positions(&body)?)?;
            positions.extend(account.positions);
            source_rows.extend(account.source_rows);
            cash += account.cash;
            account_total += account.account_total;
        }

        // Net symbols at book level after the per-account reconciliation, then
        // restore every raw wire row (including folded cash rows) as the audit surface.
        let mut holdings = Holdings {
            positions,
            cash,
            account_total,
            source_rows: vec![],
        }
        .normalized()?;
        holdings.source_rows = source_rows;
        Ok(holdings)
    }

    fn option_chain(&self, symbol: &str) -> Result<Option<OptionChain>> {
        let token = (self.token)()?;
        // The ET session date, not the machine's local one: the window's bounds are
        // expiration dates (market days), and a local date put the window a session
        // behind on a Pacific-time machine. Not a session-keyed read in the
        // `market_clock` sense; the conversion is so no adapter reads a different
        // window on a different machine.
        let query = chain_query(symbol, crate::market_clock::et_session_date(Utc::now()));
        let (status, body) = self.get(
            &format!("{}/marketdata/v1/chains?{query}", self.base),
            &token,
            "schwab option chain",
        )?;
        match status {
            200 => parse_chain(symbol, &body),
            // An un-optioned or unknown symbol is a market fact, not a fault
            // (`docs/schwab-integration.md §Failure posture`).
            404 => Ok(None),
            // An auth/server fault is a real error, not "no chain": the Portfolio job
            // records it as a gap rather than silently blanking the signal
            // (`docs/schwab-integration.md §Failure posture`).
            other => bail!("Schwab option-chain request failed (HTTP {other})"),
        }
    }

    fn option_chain_at_strike(
        &self,
        symbol: &str,
        strike: f64,
        from: &str,
        to: &str,
    ) -> Result<Option<OptionChain>> {
        let token = (self.token)()?;
        // One strike across the held contracts' expiry window, so the activity
        // signal's bounded NTM query is never widened (`docs/data-sources.md`).
        let query = format!(
            "symbol={}&contractType=ALL&strike={strike}&fromDate={from}&toDate={to}",
            encode_query(symbol),
        );
        let (status, body) = self.get(
            &format!("{}/marketdata/v1/chains?{query}", self.base),
            &token,
            "schwab option chain (overlay strike)",
        )?;
        match status {
            200 => parse_chain(symbol, &body),
            404 => Ok(None),
            other => bail!("Schwab targeted option-chain request failed (HTTP {other})"),
        }
    }

    fn supports_targeted_chain(&self) -> bool {
        true
    }
}

/// A near-the-money strike band plus a near-dated expiration window, so the fetch
/// can't balloon on a heavily optioned name.
fn chain_query(symbol: &str, today: NaiveDate) -> String {
    let to = today + Duration::days(CHAIN_WINDOW_DAYS);
    format!(
        "symbol={}&contractType=ALL&strikeCount={CHAIN_STRIKE_COUNT}&range=NTM&fromDate={}&toDate={}",
        encode_query(symbol),
        today.format("%Y-%m-%d"),
        to.format("%Y-%m-%d"),
    )
}

/// Percent-encode a symbol for a query value.
fn encode_query(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The plaintext account number is deliberately ignored; only the hash is used for
/// account calls.
fn parse_account_hashes(body: &str) -> Result<Vec<String>> {
    let json: Value = serde_json::from_str(body).context("parsing Schwab account list")?;
    let arr = json
        .as_array()
        .ok_or_else(|| anyhow!("Schwab account list was not an array"))?;
    Ok(arr
        .iter()
        .filter_map(|a| a.get("hashValue").and_then(Value::as_str))
        .map(str::to_string)
        .collect())
}

/// One account's wire values before cash-position reconciliation.
struct ParsedAccount {
    positions: Vec<Position>,
    cash_balance: f64,
    liquidation_value: Option<f64>,
}

/// One account after explicit cash rows have been folded into its cash bucket.
#[derive(Debug)]
struct ReconciledAccount {
    positions: Vec<Position>,
    source_rows: Vec<Position>,
    cash: f64,
    account_total: f64,
}

/// `cost_basis = averagePrice × quantity`, and `current_price` is derived from market
/// value so it stays consistent with it.
fn parse_positions(body: &str) -> Result<ParsedAccount> {
    let json: Value = serde_json::from_str(body).context("parsing Schwab positions")?;
    let account = json
        .get("securitiesAccount")
        .ok_or_else(|| anyhow!("Schwab positions response had no securitiesAccount"))?;

    let cash_balance = account
        .get("currentBalances")
        .and_then(|b| b.get("cashBalance"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let liquidation_value = account
        .get("currentBalances")
        .and_then(|b| b.get("liquidationValue"))
        .and_then(Value::as_f64);

    let mut positions = Vec::new();
    if let Some(rows) = account.get("positions").and_then(Value::as_array) {
        for row in rows {
            let Some(instrument) = row.get("instrument") else {
                continue;
            };
            let Some(symbol) = instrument.get("symbol").and_then(Value::as_str) else {
                continue;
            };
            let long_qty = row.get("longQuantity").and_then(Value::as_f64).unwrap_or(0.0);
            let short_qty = row
                .get("shortQuantity")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let quantity = long_qty - short_qty;
            let average_price = row.get("averagePrice").and_then(Value::as_f64).unwrap_or(0.0);
            let market_value = row.get("marketValue").and_then(Value::as_f64).unwrap_or(0.0);
            let current_price = if quantity != 0.0 {
                Some(market_value / quantity)
            } else {
                None
            };
            positions.push(Position {
                symbol: symbol.to_string(),
                description: instrument
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                asset_class: map_asset_class(
                    instrument.get("assetType").and_then(Value::as_str),
                ),
                quantity,
                cost_basis: average_price * quantity,
                market_value,
                current_price,
            });
        }
    }
    Ok(ParsedAccount {
        positions,
        cash_balance,
        liquidation_value,
    })
}

/// A `CASH_EQUIVALENT` / `CURRENCY` row may already be included in `cashBalance`.
/// `liquidationValue` distinguishes that shape from one where the row is additional
/// cash: the residual after non-cash positions must match either `cashBalance` or
/// `cashBalance + cash rows`. Ambiguous input fails the pull instead of silently
/// choosing a denominator.
fn reconcile_account(account: ParsedAccount) -> Result<ReconciledAccount> {
    let source_rows = account.positions;
    let positions: Vec<Position> = source_rows
        .iter()
        .filter(|p| p.asset_class != AssetClass::Cash)
        .cloned()
        .collect();
    let non_cash_value = positions.iter().map(|p| p.market_value).sum::<f64>();
    let cash_row_value = source_rows
        .iter()
        .filter(|p| p.asset_class == AssetClass::Cash)
        .map(|p| p.market_value)
        .sum::<f64>();
    if !(account.cash_balance.is_finite()
        && non_cash_value.is_finite()
        && cash_row_value.is_finite())
    {
        bail!("Schwab account cash reconciliation overflowed");
    }

    let has_cash_rows = source_rows
        .iter()
        .any(|p| p.asset_class == AssetClass::Cash);
    let (cash, account_total) = match account.liquidation_value {
        Some(total) if total.is_finite() => {
            if !has_cash_rows {
                (account.cash_balance, total)
            } else {
                let residual = total - non_cash_value;
                let cash_plus_rows = account.cash_balance + cash_row_value;
                if !(residual.is_finite() && cash_plus_rows.is_finite()) {
                    bail!("Schwab account cash reconciliation overflowed");
                }
                if currency_totals_match(residual, account.cash_balance) {
                    // The explicit cash row is already represented by cashBalance.
                    (account.cash_balance, total)
                } else if currency_totals_match(residual, cash_plus_rows) {
                    // The explicit row is additional cash exposure.
                    (cash_plus_rows, total)
                } else {
                    bail!(
                        "Schwab account cash rows could not be reconciled with cashBalance and \
                         liquidationValue"
                    );
                }
            }
        }
        Some(_) => bail!("Schwab account liquidationValue was not finite"),
        None if has_cash_rows && !currency_totals_match(cash_row_value, 0.0) => {
            bail!(
                "Schwab account returned cash-position rows without liquidationValue; overlap \
                 with cashBalance is ambiguous"
            );
        }
        None => {
            let total = non_cash_value + account.cash_balance;
            if !total.is_finite() {
                bail!("Schwab account total overflowed");
            }
            (account.cash_balance, total)
        }
    };

    Ok(ReconciledAccount {
        positions,
        source_rows,
        cash,
        account_total,
    })
}

/// Currency totals can differ by a cent of wire rounding; the relative rider
/// keeps the comparison stable for very large accounts without masking a row.
fn currency_totals_match(a: f64, b: f64) -> bool {
    let tolerance = 0.01_f64.max(a.abs().max(b.abs()) * 1e-10);
    (a - b).abs() <= tolerance
}

/// Unknown or absent types are `Other` (not-rated) rather than a guessed grade.
fn map_asset_class(asset_type: Option<&str>) -> AssetClass {
    match asset_type.unwrap_or("") {
        "EQUITY" => AssetClass::Stock,
        "ETF" | "COLLECTIVE_INVESTMENT" => AssetClass::Etf,
        "MUTUAL_FUND" => AssetClass::MutualFund,
        "OPTION" => AssetClass::OptionContract,
        "FIXED_INCOME" | "BOND" => AssetClass::FixedIncome,
        "CASH_EQUIVALENT" | "CURRENCY" => AssetClass::Cash,
        _ => AssetClass::Other,
    }
}

/// Flatten `callExpDateMap` / `putExpDateMap` (`date:dte → strike → [contract]`) into
/// one contract list. `Ok(None)` when a well-formed response carries no contracts; a
/// malformed or drifted body is an `Err`, so provider API drift surfaces rather than
/// masquerading as "no options listed".
fn parse_chain(symbol: &str, body: &str) -> Result<Option<OptionChain>> {
    let json: Value = serde_json::from_str(body).context("parsing Schwab option chain")?;
    // A well-formed SUCCESS response carries both expiration maps (empty objects for an
    // un-optioned name), so both absent is a drifted shape or a non-SUCCESS payload.
    // The guard is deliberately both-absent, not either-absent: the always-both-maps
    // invariant is not yet live-confirmed, so a one-sided response is parsed for the
    // map it carries. Tighten once a live response confirms the invariant.
    let has_call_map = json.get("callExpDateMap").is_some_and(Value::is_object);
    let has_put_map = json.get("putExpDateMap").is_some_and(Value::is_object);
    if !has_call_map && !has_put_map {
        bail!("unexpected Schwab option-chain response shape (no expiration maps)");
    }
    let mut contracts = Vec::new();
    collect_contracts(json.get("callExpDateMap"), OptionKind::Call, &mut contracts)?;
    collect_contracts(json.get("putExpDateMap"), OptionKind::Put, &mut contracts)?;
    if contracts.is_empty() {
        return Ok(None);
    }
    let underlying = json
        .get("symbol")
        .and_then(Value::as_str)
        .unwrap_or(symbol)
        .to_string();
    let underlying_price = json
        .get("underlyingPrice")
        .and_then(Value::as_f64)
        .filter(|p| *p > 0.0);
    Ok(Some(OptionChain {
        underlying,
        underlying_price,
        contracts,
    }))
}

/// Walk one expiration map (`{ "2026-07-17:5": { "195.0": [ {contract}, … ] } }`) into
/// `OptionQuote`s tagged `kind`. A non-numeric strike / volume / open-interest is an
/// `Err`: a fabricated 0.0 level must never enter the signal as data
/// (`docs/schwab-integration.md §Failure posture`). The IV sentinel keeps its tolerant
/// read.
fn collect_contracts(
    map: Option<&Value>,
    kind: OptionKind,
    out: &mut Vec<OptionQuote>,
) -> Result<()> {
    let Some(value) = map else {
        return Ok(());
    };
    // Absence is the tolerated one-sided case; a present non-object map is drift.
    let Some(exp_map) = value.as_object() else {
        bail!("an expiration map is present but not an object — malformed or drifted response");
    };
    for (date_key, strikes) in exp_map {
        // The map key is `date:daysToExpiration`; the ISO date is the part before ':'.
        let expiry = date_key.split(':').next().unwrap_or(date_key).to_string();
        let Some(strike_map) = strikes.as_object() else {
            bail!("strike map under {date_key:?} is not an object — malformed or drifted response");
        };
        for contracts in strike_map.values() {
            let Some(list) = contracts.as_array() else {
                bail!(
                    "contract list under {date_key:?} is not an array — malformed or drifted response"
                );
            };
            for c in list {
                let field = |key: &str| {
                    c.get(key).and_then(Value::as_f64).with_context(|| {
                        format!(
                            "a contract under {date_key:?} carries no numeric {key} — malformed or drifted response"
                        )
                    })
                };
                let strike = field("strikePrice")?;
                let volume = field("totalVolume")?;
                let open_interest = field("openInterest")?;
                // Schwab reports volatility as a percent, with -999 as "no value".
                let implied_volatility = c
                    .get("volatility")
                    .and_then(Value::as_f64)
                    .filter(|v| *v >= 0.0)
                    .map(|v| v / 100.0);
                // Delta is already scaled; anything outside [-1, 1] is the -999
                // sentinel family, never a greek.
                let delta = c
                    .get("delta")
                    .and_then(Value::as_f64)
                    .filter(|v| v.abs() <= 1.0);
                out.push(OptionQuote {
                    kind,
                    strike,
                    expiry: expiry.clone(),
                    volume,
                    open_interest,
                    implied_volatility,
                    delta,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::MockHttp;
    use crate::test_http::Canned;

    fn static_token() -> TokenProvider {
        Arc::new(|| Ok("test-access-token".to_string()))
    }

    const ACCOUNT_NUMBERS_JSON: &str =
        r#"[{"accountNumber":"12345678","hashValue":"HASH-ABC"}]"#;

    const POSITIONS_JSON: &str = r#"{
      "securitiesAccount": {
        "accountNumber": "12345678",
        "positions": [
          {
            "longQuantity": 100,
            "shortQuantity": 0,
            "averagePrice": 140.0,
            "marketValue": 19500.0,
            "instrument": {"assetType": "EQUITY", "symbol": "AAPL", "description": "APPLE INC"}
          }
        ],
        "currentBalances": {"cashBalance": 10000.0, "liquidationValue": 29500.0}
      }
    }"#;

    const POSITIONS_WITH_DUPLICATED_SWEEP_JSON: &str = r#"{
      "securitiesAccount": {
        "accountNumber": "12345678",
        "positions": [
          {
            "longQuantity": 100,
            "shortQuantity": 0,
            "averagePrice": 140.0,
            "marketValue": 19500.0,
            "instrument": {"assetType": "EQUITY", "symbol": "AAPL", "description": "APPLE INC"}
          },
          {
            "longQuantity": 10000,
            "shortQuantity": 0,
            "averagePrice": 1.0,
            "marketValue": 10000.0,
            "instrument": {"assetType": "CASH_EQUIVALENT", "symbol": "SWEEP", "description": "BANK SWEEP"}
          }
        ],
        "currentBalances": {"cashBalance": 10000.0, "liquidationValue": 29500.0}
      }
    }"#;

    #[test]
    fn parse_account_hashes_takes_hash_not_plaintext() {
        let hashes = parse_account_hashes(ACCOUNT_NUMBERS_JSON).unwrap();
        assert_eq!(hashes, vec!["HASH-ABC".to_string()]);
    }

    #[test]
    fn parse_positions_maps_to_dtos_with_currency_totals() {
        let account = parse_positions(POSITIONS_JSON).unwrap();
        assert_eq!(account.cash_balance, 10_000.0);
        assert_eq!(account.liquidation_value, Some(29_500.0));
        assert_eq!(account.positions.len(), 1);
        let p = &account.positions[0];
        assert_eq!(p.symbol, "AAPL");
        assert_eq!(p.asset_class, AssetClass::Stock);
        assert_eq!(p.quantity, 100.0);
        assert_eq!(p.cost_basis, 14_000.0); // averagePrice 140 × 100
        assert_eq!(p.market_value, 19_500.0);
        assert_eq!(p.current_price, Some(195.0)); // 19_500 / 100
    }

    #[test]
    fn duplicated_bank_sweep_is_folded_into_cash_once_and_retained_for_audit() {
        let account =
            reconcile_account(parse_positions(POSITIONS_WITH_DUPLICATED_SWEEP_JSON).unwrap())
                .unwrap();

        assert_eq!(account.positions.len(), 1);
        assert_eq!(account.positions[0].symbol, "AAPL");
        assert_eq!(account.cash, 10_000.0);
        assert_eq!(account.account_total, 29_500.0);
        assert_eq!(account.source_rows.len(), 2);
        assert_eq!(account.source_rows[1].symbol, "SWEEP");
    }

    #[test]
    fn separately_reported_cash_row_is_added_to_cash_once() {
        let mut account = parse_positions(POSITIONS_WITH_DUPLICATED_SWEEP_JSON).unwrap();
        account.liquidation_value = Some(39_500.0);
        let account = reconcile_account(account).unwrap();

        assert_eq!(account.positions.len(), 1);
        assert_eq!(account.cash, 20_000.0);
        assert_eq!(account.account_total, 39_500.0);
        assert_eq!(account.source_rows.len(), 2);
    }

    #[test]
    fn irreconcilable_cash_row_fails_instead_of_choosing_the_nearest_total() {
        let mut account = parse_positions(POSITIONS_WITH_DUPLICATED_SWEEP_JSON).unwrap();
        account.liquidation_value = Some(30_000.0);
        let err = reconcile_account(account).unwrap_err().to_string();
        assert!(err.contains("could not be reconciled"), "{err}");
    }

    #[test]
    fn cash_row_without_a_liquidation_total_is_ambiguous_not_double_counted() {
        let mut account = parse_positions(POSITIONS_WITH_DUPLICATED_SWEEP_JSON).unwrap();
        account.liquidation_value = None;
        let err = reconcile_account(account).unwrap_err().to_string();
        assert!(err.contains("without liquidationValue"), "{err}");
    }

    #[test]
    fn map_asset_class_covers_the_known_types_and_defaults_to_other() {
        assert_eq!(map_asset_class(Some("EQUITY")), AssetClass::Stock);
        assert_eq!(map_asset_class(Some("COLLECTIVE_INVESTMENT")), AssetClass::Etf);
        assert_eq!(map_asset_class(Some("OPTION")), AssetClass::OptionContract);
        assert_eq!(map_asset_class(Some("WEIRD")), AssetClass::Other);
        assert_eq!(map_asset_class(None), AssetClass::Other);
    }

    #[test]
    fn parse_chain_flattens_both_maps_and_scales_iv() {
        let body = r#"{
          "symbol": "AAPL",
          "underlyingPrice": 195.0,
          "callExpDateMap": {"2026-07-17:5": {"195.0": [
            {"putCall":"CALL","strikePrice":195.0,"totalVolume":4000,"openInterest":12000,"volatility":27.0,"delta":0.52}
          ]}},
          "putExpDateMap": {"2026-07-17:5": {"185.0": [
            {"putCall":"PUT","strikePrice":185.0,"totalVolume":3100,"openInterest":9500,"volatility":-999.0,"delta":-999.0}
          ]}}
        }"#;
        let chain = parse_chain("AAPL", body).unwrap().expect("chain present");
        assert_eq!(chain.underlying, "AAPL");
        assert_eq!(chain.underlying_price, Some(195.0));
        assert_eq!(chain.contracts.len(), 2);
        let call = chain.contracts.iter().find(|c| c.kind == OptionKind::Call).unwrap();
        assert_eq!(call.strike, 195.0);
        assert_eq!(call.implied_volatility, Some(0.27)); // 27% → 0.27
        assert_eq!(call.delta, Some(0.52));
        let put = chain.contracts.iter().find(|c| c.kind == OptionKind::Put).unwrap();
        assert_eq!(put.implied_volatility, None); // -999 sentinel → no value
        assert_eq!(put.delta, None); // -999 sentinel → no greek
        let bare = r#"{"symbol":"AAPL","callExpDateMap":{"2026-07-17:5":{"195.0":[
            {"putCall":"CALL","strikePrice":195.0,"totalVolume":10,"openInterest":5,"volatility":25.0}
        ]}},"putExpDateMap":{}}"#;
        let chain = parse_chain("AAPL", bare).unwrap().unwrap();
        assert_eq!(chain.contracts[0].delta, None);
    }

    #[test]
    fn targeted_strike_fetch_scopes_the_query_and_parses_deltas() {
        let body = r#"{"symbol":"AAPL","underlyingPrice":195.0,
          "callExpDateMap": {"2027-01-15:512": {"210.0": [
            {"putCall":"CALL","strikePrice":210.0,"totalVolume":10,"openInterest":500,"volatility":31.0,"delta":0.41}
          ]}},
          "putExpDateMap": {}
        }"#;
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body,
        }]);
        let source = SchwabApiSource::with_base_url(
            server.base_url.trim_end_matches('/').to_string(),
            static_token(),
        );
        let chain = source
            .option_chain_at_strike("AAPL", 210.0, "2027-01-15", "2027-01-15")
            .unwrap()
            .expect("targeted chain present");
        assert_eq!(chain.contracts.len(), 1);
        assert_eq!(chain.contracts[0].delta, Some(0.41));
        let targets = server.request_targets();
        assert!(targets[0].contains("strike=210"), "{targets:?}");
        assert!(targets[0].contains("fromDate=2027-01-15"), "{targets:?}");
        assert!(!targets[0].contains("range=NTM"), "{targets:?}");
        assert!(source.supports_targeted_chain());
        assert!(!crate::schwab::FixtureHoldingsSource::new().supports_targeted_chain());
    }

    #[test]
    fn parse_chain_none_when_no_contracts() {
        assert!(
            parse_chain("AAPL", r#"{"symbol":"AAPL","callExpDateMap":{},"putExpDateMap":{}}"#)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn parse_chain_errors_on_a_non_numeric_contract_field() {
        let body = r#"{"symbol":"AAPL","callExpDateMap":{"2026-09-18:44":{"195.0":[
            {"putCall":"CALL","strikePrice":"195.0","totalVolume":10,"openInterest":5,"volatility":30.0}
        ]}},"putExpDateMap":{}}"#;
        assert!(parse_chain("AAPL", body).is_err());
        let body = r#"{"symbol":"AAPL","callExpDateMap":{"2026-09-18:44":{"195.0":[
            {"putCall":"CALL","strikePrice":195.0,"openInterest":5,"volatility":30.0}
        ]}},"putExpDateMap":{}}"#;
        assert!(parse_chain("AAPL", body).is_err(), "a missing volume must not read as zero");
    }

    #[test]
    fn parse_chain_errors_on_a_present_non_object_map() {
        let body = r#"{"symbol":"AAPL","callExpDateMap":{"2026-09-18:44":{"195.0":[
            {"putCall":"CALL","strikePrice":195.0,"totalVolume":10,"openInterest":5,"volatility":30.0}
        ]}},"putExpDateMap":[]}"#;
        assert!(parse_chain("AAPL", body).is_err());
    }

    #[test]
    fn parse_chain_malformed_body_is_an_error_not_a_silent_gap() {
        assert!(parse_chain("AAPL", "{not json").is_err());
    }

    #[test]
    fn parse_chain_valid_json_without_expiration_maps_is_an_error() {
        assert!(parse_chain("AAPL", r#"{"symbol":"AAPL","status":"FAILED"}"#).is_err());
        assert!(parse_chain("AAPL", r#"{"symbol":"AAPL","callMap":{},"putMap":{}}"#).is_err());
    }

    #[test]
    fn parse_chain_tolerates_a_single_sided_map() {
        // Pins the deliberately lenient one-sided read (see `parse_chain`), so a
        // tightening to either-absent is a conscious edit.
        let body = r#"{"symbol":"AAPL","callExpDateMap":{"2026-07-17:5":{"195.0":[
            {"putCall":"CALL","strikePrice":195.0,"totalVolume":10,"openInterest":20,"volatility":25.0}
        ]}}}"#;
        let chain = parse_chain("AAPL", body)
            .unwrap()
            .expect("a single-sided chain still parses");
        assert_eq!(chain.contracts.len(), 1);
        assert_eq!(chain.contracts[0].kind, OptionKind::Call);
    }

    #[test]
    fn chain_query_bounds_strikes_and_expiration_window() {
        let q = chain_query("SPY", NaiveDate::from_ymd_opt(2026, 7, 2).unwrap());
        assert!(q.contains("symbol=SPY"), "{q}");
        assert!(q.contains("strikeCount=12"), "{q}");
        assert!(q.contains("range=NTM"), "{q}");
        assert!(q.contains("contractType=ALL"), "{q}");
        assert!(q.contains("fromDate=2026-07-02"), "{q}");
        // +60 days from 2026-07-02.
        assert!(q.contains("toDate=2026-08-31"), "{q}");
    }

    #[test]
    fn option_chain_404_is_a_quiet_no_options_read_but_a_fault_is_an_error() {
        let not_found = MockHttp::serve(vec![Canned::Reply {
            status: 404,
            headers: vec![],
            body: "not found",
        }]);
        let source = SchwabApiSource::with_base_url(
            not_found.base_url.trim_end_matches('/').to_string(),
            static_token(),
        );
        assert!(source.option_chain("NOPE").unwrap().is_none());

        let unauthorized = MockHttp::serve(vec![Canned::Reply {
            status: 401,
            headers: vec![],
            body: "unauthorized",
        }]);
        let source = SchwabApiSource::with_base_url(
            unauthorized.base_url.trim_end_matches('/').to_string(),
            static_token(),
        );
        assert!(source.option_chain("AAPL").is_err());
    }

    #[test]
    fn holdings_resolves_hash_then_pulls_positions_get_only() {
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![("Content-Type", "application/json")],
                body: ACCOUNT_NUMBERS_JSON,
            },
            Canned::Reply {
                status: 200,
                headers: vec![("Content-Type", "application/json")],
                body: POSITIONS_WITH_DUPLICATED_SWEEP_JSON,
            },
        ]);
        let base = server.base_url.trim_end_matches('/').to_string();
        let source = SchwabApiSource::with_base_url(base, static_token());
        let holdings = source.holdings().expect("holdings pull succeeds");
        assert_eq!(holdings.positions.len(), 1);
        assert_eq!(holdings.cash, 10_000.0);
        // account_total is the account's reported liquidation value.
        assert_eq!(holdings.account_total, 29_500.0);
        assert_eq!(holdings.source_rows.len(), 2);
        assert_eq!(holdings.source_rows[1].symbol, "SWEEP");

        let paths = server.request_paths();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], "/trader/v1/accounts/accountNumbers");
        assert_eq!(paths[1], "/trader/v1/accounts/HASH-ABC");
        assert!(
            !paths.iter().any(|p| p.contains("orders")),
            "adapter must never build an order path: {paths:?}"
        );
    }
}
