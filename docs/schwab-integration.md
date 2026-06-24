# Charles Schwab Integration

Portfolio Analysis ([portfolio-analysis.md](portfolio-analysis.md)) sources the user's holdings from their Charles Schwab brokerage account via the **Schwab Trader API**, with a **manual import** fallback. Holdings are fetched **only on explicit user action** — the app never polls or auto-refreshes.

## Manual pull, never automatic

The Portfolio page has a button that re-pulls holdings on demand; there is no scheduler, timer, or background sync. The user controls when fresh data is fetched — which also means the user controls when the periodic re-authentication (below) happens, rather than it surfacing unexpectedly mid-session. The last successful pull persists, so the portfolio is viewable without re-fetching.

## Authorization

Access uses Schwab's three-legged **OAuth 2.0 authorization-code** flow. The user authenticates in the system browser with their Schwab **brokerage** credentials (not developer-portal credentials) and selects which linked accounts to grant. The app captures the redirect on a short-lived **self-signed HTTPS loopback server at `https://127.0.0.1:8182`** — Schwab requires HTTPS and an exact callback match, and only the loopback host is permitted, so no hosted backend is needed. Because no certificate authority signs a `127.0.0.1` certificate, the browser shows a one-time self-signed-cert warning the user clicks through.

## Token lifecycle

- **Access token: 30 minutes**, refreshed automatically by the app as needed.
- **Refresh token: 7 days, and it cannot be extended.** When it expires, the only recovery is to repeat the interactive browser login.

The unavoidable consequence is a **weekly re-login** — designed for rather than hidden: the app prompts for re-authentication once the refresh token has lapsed, and the manual-import path (below) keeps the feature usable in the meantime. Tokens are stored locally with the app's other credentials (see [configuration.md](configuration.md)) and are never displayed.

## What is pulled

Holdings come from the account positions endpoint (`GET /trader/v1/accounts/{accountHash}?fields=positions`). Schwab identifies accounts by a **hashed account number**, not the plaintext one, so the app first resolves the plaintext→hash mapping and uses the hash for all account calls. Each position yields the fields the analysis needs: instrument identity (symbol, CUSIP, asset type), quantity, average cost (cost basis), market value, and profit/loss.

## Fundamentals stay with FMP

Schwab's fundamentals are thin summary ratios with an undocumented, unstable shape, and there is no financial-statement (income / balance-sheet / cash-flow) endpoint. So **Schwab is the source of truth for holdings only**; the deeper company financials a holding's analysis needs come from **FMP**, which the app already integrates ([data-sources.md](data-sources.md)). The two are complementary: Schwab says *what you own and at what cost*; FMP says *how the company is doing*.

## Manual import fallback

Holdings can also be entered **manually — by pasting or importing a CSV** of symbols, quantities, and cost bases. Both ingestion paths populate the same internal holdings model behind one trait, so the analysis pipeline is agnostic to where holdings came from. Manual import covers three real cases: the few-day window while a new Schwab developer app awaits approval, any lapse of the 7-day refresh token before re-login, and use without linking a brokerage account at all.

## Failure posture

A failed or unauthorized pull leaves the last good holdings intact — it never clears or corrupts stored positions. Holdings availability from *either* source is the precondition the Portfolio Analysis job gates on; with no holdings, the job does not run (see [portfolio-analysis.md](portfolio-analysis.md)).
