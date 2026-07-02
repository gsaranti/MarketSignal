//! The live Schwab Trader API holdings source (`docs/schwab-integration.md §What is
//! pulled`). Implements the same [`HoldingsSource`] the fixture does, so nothing
//! downstream of the seam changes — the Portfolio pipeline never learns whether the
//! holdings came from the fixture or the wire.
//!
//! **Read-only by construction.** This adapter builds *only* `GET`s against the
//! positions, account-list, and option-chain endpoints — it never constructs an
//! order/trading request. That is the code-enforced half of the safety boundary the
//! docs describe (the Trader API has no read-only token scope, so the guarantee lives
//! here, not in a credential): the module exposes no order path, and the GET-only test
//! pins it.
//!
//! Access tokens are supplied by a [`TokenProvider`] closure (over
//! [`crate::schwab_oauth::OauthClient`] in production, a canned token in tests), so the
//! refresh lifecycle stays in `schwab_oauth` and the wire-mapping here is unit-testable
//! against a localhost mock with no OAuth flow. The token rides an `Authorization`
//! header and never reaches a log line or the run tracker.

use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use crate::http_retry::send_with_retry;
use crate::portfolio::AssetClass;
use crate::schwab::{Holdings, HoldingsSource, OptionChain, OptionKind, OptionQuote, Position};

/// Schwab's API host — both the Trader (`/trader/v1`) and Market Data
/// (`/marketdata/v1`) products live under it.
const SCHWAB_API_BASE: &str = "https://api.schwabapi.com";

/// Supplies a currently-valid access token for one API call. In production this
/// closes over the OAuth client's `valid_access_token` (which refreshes as needed);
/// tests hand in a fixed token.
pub type TokenProvider = Arc<dyn Fn() -> Result<String> + Send + Sync>;

/// The live holdings source: a blocking HTTP client, the API base URL (overridable for
/// tests), and the access-token provider.
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

    /// A bearer-authorized GET through the shared retry/backoff. Returns the final
    /// `(status, body)` for the caller to interpret. The token is applied as an
    /// `Authorization` header — never placed in the URL or the error context.
    fn get(&self, url: &str, token: &str, label: &str) -> Result<(u16, String)> {
        send_with_retry(label, || self.http.get(url).bearer_auth(token))
    }
}

impl HoldingsSource for SchwabApiSource {
    fn holdings(&self) -> Result<Holdings> {
        let token = (self.token)()?;

        // Schwab identifies accounts by an opaque hash, not the plaintext number, so
        // resolve the plaintext→hash map first and use the hash for the position calls.
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

        // Aggregate every granted account into one holdings snapshot: positions
        // concatenated, cash summed. account_total is derived (Σ market value + cash),
        // not read from a balance field, so the fixture invariant the engine relies on
        // holds regardless of source.
        let mut positions: Vec<Position> = Vec::new();
        let mut cash = 0.0;
        for hash in hashes {
            let (status, body) = self.get(
                &format!("{}/trader/v1/accounts/{hash}?fields=positions", self.base),
                &token,
                "schwab positions",
            )?;
            if status != 200 {
                bail!("Schwab positions request failed (HTTP {status})");
            }
            let (mut ps, account_cash) = parse_positions(&body)?;
            positions.append(&mut ps);
            cash += account_cash;
        }

        let account_total = positions.iter().map(|p| p.market_value).sum::<f64>() + cash;
        Ok(Holdings {
            positions,
            cash,
            account_total,
        })
    }

    fn option_chain(&self, symbol: &str) -> Result<Option<OptionChain>> {
        let token = (self.token)()?;
        let (status, body) = self.get(
            &format!(
                "{}/marketdata/v1/chains?symbol={}",
                self.base,
                encode_query(symbol)
            ),
            &token,
            "schwab option chain",
        )?;
        // Fail-soft: a missing or unlisted chain degrades this stock's options signal to
        // a gap, never a whole-job failure (`docs/schwab-integration.md §Failure
        // posture`). The holdings pull already validated the token, so a non-200 here is
        // treated as "no chain for this symbol".
        if status != 200 {
            return Ok(None);
        }
        Ok(parse_chain(symbol, &body))
    }
}

/// Percent-encode a symbol for a query value. Ticker symbols are alphanumeric plus a
/// few punctuation characters (`.` / `-`), so only the rest need escaping.
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

/// Extract the account *hash* values from Schwab's `accountNumbers` response — an array
/// of `{accountNumber, hashValue}`. The plaintext number is deliberately ignored; only
/// the hash is used for account calls.
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

/// Map one account's positions response to our [`Position`] list plus its cash balance.
/// Cost basis and current price follow the account-currency-total convention the DTOs
/// document: `cost_basis = averagePrice × quantity`, and `current_price` is derived from
/// market value so it stays consistent with it.
fn parse_positions(body: &str) -> Result<(Vec<Position>, f64)> {
    let json: Value = serde_json::from_str(body).context("parsing Schwab positions")?;
    let account = json
        .get("securitiesAccount")
        .ok_or_else(|| anyhow!("Schwab positions response had no securitiesAccount"))?;

    let cash = account
        .get("currentBalances")
        .and_then(|b| b.get("cashBalance"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);

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
    Ok((positions, cash))
}

/// Map Schwab's `assetType` string to our [`AssetClass`]. Unknown or absent types are
/// `Other` (not-rated) rather than a guessed grade.
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

/// Map Schwab's `/chains` response to our [`OptionChain`], flattening the nested
/// `callExpDateMap` / `putExpDateMap` (`date:dte → strike → [contract]`) into a flat
/// contract list. Returns `None` when the response carries no contracts — a name with
/// no listed options, exactly the gap the fixture and the failure posture describe.
fn parse_chain(symbol: &str, body: &str) -> Option<OptionChain> {
    let json: Value = serde_json::from_str(body).ok()?;
    let mut contracts = Vec::new();
    collect_contracts(json.get("callExpDateMap"), OptionKind::Call, &mut contracts);
    collect_contracts(json.get("putExpDateMap"), OptionKind::Put, &mut contracts);
    if contracts.is_empty() {
        return None;
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
    Some(OptionChain {
        underlying,
        underlying_price,
        contracts,
    })
}

/// Walk one expiration map (`{ "2026-07-17:5": { "195.0": [ {contract}, … ] } }`) into
/// `OptionQuote`s, tagging each with `kind`.
fn collect_contracts(map: Option<&Value>, kind: OptionKind, out: &mut Vec<OptionQuote>) {
    let Some(exp_map) = map.and_then(Value::as_object) else {
        return;
    };
    for (date_key, strikes) in exp_map {
        // The map key is `date:daysToExpiration`; the ISO date is the part before ':'.
        let expiry = date_key.split(':').next().unwrap_or(date_key).to_string();
        let Some(strike_map) = strikes.as_object() else {
            continue;
        };
        for contracts in strike_map.values() {
            let Some(list) = contracts.as_array() else {
                continue;
            };
            for c in list {
                let strike = c.get("strikePrice").and_then(Value::as_f64).unwrap_or(0.0);
                let volume = c.get("totalVolume").and_then(Value::as_f64).unwrap_or(0.0);
                let open_interest = c.get("openInterest").and_then(Value::as_f64).unwrap_or(0.0);
                // Schwab reports volatility as a percent, with -999 as "no value".
                let implied_volatility = c
                    .get("volatility")
                    .and_then(Value::as_f64)
                    .filter(|v| *v >= 0.0)
                    .map(|v| v / 100.0);
                out.push(OptionQuote {
                    kind,
                    strike,
                    expiry: expiry.clone(),
                    volume,
                    open_interest,
                    implied_volatility,
                });
            }
        }
    }
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

    #[test]
    fn parse_account_hashes_takes_hash_not_plaintext() {
        let hashes = parse_account_hashes(ACCOUNT_NUMBERS_JSON).unwrap();
        assert_eq!(hashes, vec!["HASH-ABC".to_string()]);
    }

    #[test]
    fn parse_positions_maps_to_dtos_with_currency_totals() {
        let (positions, cash) = parse_positions(POSITIONS_JSON).unwrap();
        assert_eq!(cash, 10_000.0);
        assert_eq!(positions.len(), 1);
        let p = &positions[0];
        assert_eq!(p.symbol, "AAPL");
        assert_eq!(p.asset_class, AssetClass::Stock);
        assert_eq!(p.quantity, 100.0);
        assert_eq!(p.cost_basis, 14_000.0); // averagePrice 140 × 100
        assert_eq!(p.market_value, 19_500.0);
        assert_eq!(p.current_price, Some(195.0)); // 19_500 / 100
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
            {"putCall":"CALL","strikePrice":195.0,"totalVolume":4000,"openInterest":12000,"volatility":27.0}
          ]}},
          "putExpDateMap": {"2026-07-17:5": {"185.0": [
            {"putCall":"PUT","strikePrice":185.0,"totalVolume":3100,"openInterest":9500,"volatility":-999.0}
          ]}}
        }"#;
        let chain = parse_chain("AAPL", body).expect("chain present");
        assert_eq!(chain.underlying, "AAPL");
        assert_eq!(chain.underlying_price, Some(195.0));
        assert_eq!(chain.contracts.len(), 2);
        let call = chain.contracts.iter().find(|c| c.kind == OptionKind::Call).unwrap();
        assert_eq!(call.strike, 195.0);
        assert_eq!(call.implied_volatility, Some(0.27)); // 27% → 0.27
        let put = chain.contracts.iter().find(|c| c.kind == OptionKind::Put).unwrap();
        assert_eq!(put.implied_volatility, None); // -999 sentinel → no value
    }

    #[test]
    fn parse_chain_none_when_no_contracts() {
        assert!(parse_chain("AAPL", r#"{"symbol":"AAPL","callExpDateMap":{},"putExpDateMap":{}}"#).is_none());
    }

    #[test]
    fn holdings_resolves_hash_then_pulls_positions_get_only() {
        // Two replies: the account-list GET, then the positions GET for the hash.
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 200,
                headers: vec![("Content-Type", "application/json")],
                body: ACCOUNT_NUMBERS_JSON,
            },
            Canned::Reply {
                status: 200,
                headers: vec![("Content-Type", "application/json")],
                body: POSITIONS_JSON,
            },
        ]);
        let base = server.base_url.trim_end_matches('/').to_string();
        let source = SchwabApiSource::with_base_url(base, static_token());
        let holdings = source.holdings().expect("holdings pull succeeds");
        assert_eq!(holdings.positions.len(), 1);
        assert_eq!(holdings.cash, 10_000.0);
        // account_total is derived: Σ market value + cash.
        assert_eq!(holdings.account_total, 29_500.0);

        // GET-only: the paths hit are the account-list and the hash's positions — no
        // order/trading path is ever built.
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
