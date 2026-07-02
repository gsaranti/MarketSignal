# Charles Schwab Integration

The local suite sources the user's holdings — and equity option chains — from their Charles Schwab brokerage account via the **Schwab Trader API**. **A connected Schwab account is required to run either local job** (see [§A connected Schwab account is required](#a-connected-schwab-account-is-required)); manual import is a supplement, not a substitute. Data is fetched **only on explicit user action** — the app never polls or auto-refreshes.

## Manual pull, never automatic

The Portfolio page has a button that re-pulls holdings on demand; there is no scheduler, timer, or background sync. The user controls when fresh data is fetched — which also means the user controls when the periodic re-authentication (below) happens, rather than it surfacing unexpectedly mid-session. The last successful pull persists, so the portfolio is viewable without re-fetching.

## Authorization

Access uses Schwab's three-legged **OAuth 2.0 authorization-code** flow. The user authenticates in the system browser with their Schwab **brokerage** credentials (not developer-portal credentials) and selects which linked accounts to grant. The app captures the redirect on a short-lived **self-signed HTTPS loopback server at `https://127.0.0.1:8182`** — Schwab requires HTTPS and an exact callback match, and only the loopback host is permitted, so no hosted backend is needed. Because no certificate authority signs a `127.0.0.1` certificate, the browser shows a one-time self-signed-cert warning the user clicks through.

## Token lifecycle

- **Access token: 30 minutes**, refreshed automatically by the app as needed.
- **Refresh token: 7 days, and it cannot be extended.** When it expires, the only recovery is to repeat the interactive browser login.

The unavoidable consequence is a **weekly re-login** — designed for rather than hidden: the app prompts for re-authentication once the refresh token has lapsed, and **because a connected account is required, both local jobs are blocked until the user re-authenticates** (the accepted cost of sourcing holdings and option chains from Schwab). The app secret and the OAuth tokens are stored in the **macOS Keychain** — not the SQLite settings store used for less-sensitive API keys — because they are bearer credentials to the user's brokerage account (see [configuration.md §Charles Schwab Connection](configuration.md#charles-schwab-connection)); they are never displayed.

## What is pulled

Holdings come from the account positions endpoint (`GET /trader/v1/accounts/{accountHash}?fields=positions`). Schwab identifies accounts by a **hashed account number**, not the plaintext one, so the app first resolves the plaintext→hash mapping and uses the hash for all account calls. Each position yields the fields the analysis needs: instrument identity (symbol, CUSIP, asset type), quantity, average cost (cost basis), market value, and profit/loss.

**Option chains** come from the same OAuth's market-data endpoint (`GET /marketdata/v1/chains`) — no per-call fee, but served by the **Market Data Production** product, which is registered alongside Accounts-and-Trading on the developer app (both are attached, so chains do not 403); it is a separate product on the same OAuth, not the same Trader API surface. Each contract returns volume, open interest, implied volatility, and greeks, from which the suite computes a deterministic **options-activity signal** per stock: the put/call ratio (by volume and by open interest) and an IV/skew read (see [portfolio-analysis.md](portfolio-analysis.md), [trade-opportunities.md](trade-opportunities.md)). This is a rough **activity proxy, not positioning truth** — volume and open interest don't reveal whether contracts were bought or sold, opened or closed, or used to hedge, and deep-in-the-money exercise flow can distort the ratio — so it is interpreted with that caveat and kept separate from the grade sub-scores until calibration proves it. "Deterministic" means a canonical, documented method (expiration window, delta / moneyness bands, a liquidity floor, zero-bid exclusion; IV-skew as a matched-tenor 25-delta risk reversal) whose exact parameters are fixed at implementation. Chains are fetched **fresh at job start** (not piggybacked on the holdings pull), carry an as-of / market-state timestamp, are **rejected if stale** (mirroring the report's COT freshness guard), and the request is bounded by expiration and strike range to cap volume. Schwab serves no options history; persisted snapshots (for trend) follow the suite's run retention ([storage.md](storage.md)).

## Fundamentals stay with FMP

Schwab's fundamentals are thin summary ratios with an undocumented, unstable shape, and there is no financial-statement (income / balance-sheet / cash-flow) endpoint. So **Schwab is the source of truth for holdings and option chains, not fundamentals**; the deeper company financials a holding's analysis needs come from **FMP and SEC EDGAR** ([data-sources.md](data-sources.md)). Schwab says *what you own, at what cost, and how active its options market is*; FMP and SEC say *how the company is doing*.

## Manual import (supplement)

Holdings can also be entered **manually — by pasting or importing a CSV** of symbols, quantities, and cost bases, populating the same internal holdings model behind one trait. Because a connected Schwab account is **required** to run either job (below), manual import is a **supplement, not a substitute**: it adds positions Schwab doesn't report (for example, holdings at another brokerage) so the portfolio view can be complete. It does not bypass the Schwab-connection gate, and manually-added equities still draw their options-activity signal from Schwab chains where the symbol is listed.

## A connected Schwab account is required

A valid Schwab connection is a hard precondition for **both** local jobs — Portfolio Analysis and Trade Opportunities. Both gate on it: Portfolio Analysis because holdings come from Schwab, and Trade Opportunities because its per-candidate options-activity signal does. If Schwab is not connected — never linked, or the 7-day refresh token has lapsed — both jobs are **blocked** with a re-authentication prompt, not run in a degraded mode (see [portfolio-analysis.md](portfolio-analysis.md), [trade-opportunities.md](trade-opportunities.md), [interface.md](interface.md)). Manual-import holdings do not satisfy this gate.

## Failure posture

A failed or unauthorized pull leaves the last good holdings intact — it never clears or corrupts stored positions. A stock's per-stock options signal degrades to a gap only when Schwab returns no chain for that symbol (e.g. a name with no listed options), never as a whole-job failure.
