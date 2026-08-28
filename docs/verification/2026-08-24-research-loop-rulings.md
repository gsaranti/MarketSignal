# Research-loop slice — review rounds, fix inventory, and the 2026-08-24 channel rulings

**Date:** 2026-08-23 – 2026-08-24.
**Scope:** the live research loop (the final Portfolio Analysis slice, `portfolio-v12`), taken through eight external Codex review rounds after `/metis-review-task` approved it.
**Outcome:** approved.
Every confirmed finding was fixed in-round, and the three semantic-validation P1 classes that no deterministic check could close were resolved by **architecture ruling** (2026-08-24, decided with the reviewer): bounding consequences instead of claiming deterministic understanding of prose.

## The three channel rulings (2026-08-24)

1. **Forward assumption — shadow-only, suite-wide.**
   The engine still evaluates every `research_forward_assumption` under the app-owned Step-6e conflict policy, but the write-back is parked: the audit records the would-have target recompute ("would have moved the 12-month base target A → B") and the Step-6b targets always stand.
   The no-splice guarantee is structural — `engine_output` is immutable past Step 6b.
   Promotion waits on inspected shadow cases against a drafted numeric bar (ruled 2026-08-27; canonical at portfolio-workflow.md §Step 6e); Trade Opportunities' direct-assumption leg carries the same ruling, its `research_target_scenario` bridge (claim-by-claim validated) deliberately left binding.
2. **Leading indicator — anchor gated on keyed driver references.**
   Ledger key drivers carry an app-assigned stable `driver_id` (minted at ledger validation, carried by name across rewrites); the indicator must cite one via `confirms_driver_id`, and the app-computed `driver_verified` (never model-set) is the only thing that lets its presence suppress the narrative hype ceiling.
   An unknown or stale reference keeps the indicator as visible, gap-noted evidence.
3. **Research-fed fraud — advisory.**
   The validated claim never joins the hard-forensic state (the merge path is removed; the hard rule reads the item-classified filing kinds alone).
   It persists on the audit and reaches interpretation as clearly-labeled attention evidence; promotion back to a hard trigger has exactly two routes — a structured-field adapter or per-claim acknowledgment — confirmed 2026-08-27 as the only ones (canonical at trade-opportunities-workflow.md §Step 5c).

## Fix inventory (rounds 1–8, condensed)

- **Security / fetch boundary:** the SSRF classifier covers the special-use ranges (192.0.0.0/24, 198.18.0.0/15, TEST-NET, 240.0.0.0/4, IPv6 site-local / documentation, v4-compatible forms); cached documents re-pass the URL policy (scheme, deny list, literal-address rules) before serving.
- **Search consent:** the SearXNG health probe judges through the same rank-time filter the run uses (zero-usable-results reads degraded), and an all-filtered result set falls back to Tavily like a raw empty one.
- **Typed-channel validation:** every channel is page-grounded against this run's fetched pages — the assumption states its number (or its stated range endpoints, both printed and bounding the value), carries forward-fact language, and names the holding; the indicator's value must be stated by a third-party page (issuer identity in the host rejects — IR class, name token, ticker, or suffix-stripped acronym); the fraud claim requires a drafted regulator / court host allowlist, two distinct lexicon terms outside `anti-` constructions, and issuer identity.
- **Unit / label honesty:** assumption units are monetarily typed and magnitude-normalized (an EPS fact rejects non-per-share vocabulary and any magnitude; a bare sub-1e6 revenue value rejects as unit-ambiguous); the fact-type whitelist matches whole tokens with negating / hedging tokens disqualifying outright.
- **Identity matching:** the shared matcher is structural — a symbol matches only in ticker context (`$` or an exchange / ticker-label colon), and an issuer-name token matches only as a distinctive capitalized word, sentence-initial matches requiring the proper-noun run.
- **Pre-profit activation legs:** value corroboration holds number boundaries (decimals included, both integer renders), periods must normalize to an ISO period-end, and `published_at` must be an ISO date.
- **Distillation reconciliation:** an analyzed or dormant topic the model fails to re-emit is gap-recorded and its stale seed row deleted; a duplicate topic keeps the first object; the routing size counts the disconfirming pass's full ledger.
- **Budget honesty:** failed live fetch attempts spend the 40-attempt ceiling.
- **Frontend:** the SearXNG launch preflight updates the Settings indicator, is epoch-guarded against settings reloads and mid-flight endpoint saves (bounded re-probe, degraded-unknown fallback), and the Run triggers disable during a save; a stale pre-run notice closes when a save starts.
- **Docs:** every contract above is single-homed and the corpus swept — stale designed / dormant / stubbed claims corrected across `docs/` and both logic-flow docs, and the rulings recorded in their canonical homes (the producer contract at trade-opportunities-workflow.md §Step 5c, the shadow mode at portfolio-workflow.md §Step 6e, the driver-id gate at portfolio-analysis.md §The position thesis ledger and §Starting parameters).

## Drafted values introduced (calibratable)

The fraud host allowlist and page lexicon (two-term floor), the assumption forward-fact lexicon and negation-token list, the currency / per-share unit token sets and the 1e6 revenue floor, the generic-name-token and ticker-label lists, the probe query, and the 3-attempt launch-probe cap.

## Accepted residuals (recorded, not defects)

Semantic attribution (fraud *by* the issuer, number↔statement binding, indicator↔driver economics) is the model's reading by the engine-computes / model-interprets spine — the rulings bound its consequences instead.
The first-party host check misses aliases and nonliteral domains until an issuer-website field rides the profile (the named future lever).
A page whose only identity mark is a bare unmarked ticker rejects when no company name resolved (conservative, fail-soft).

## Gates

`cargo test` 1185/0 (31 ignored), `cargo test --features demo-run` 1185/0, `cargo clippy --all-targets --all-features` clean, `npm run build` clean, `npm test` 46/46 + 247/247.

## Disposition

The big-run watch set gains the ruling watches: shadow-assumption resolutions (inspect the would-have lines against their cited pages), unverified-driver indicator gaps, and advisory fraud claims — the shadow evidence the promotion decisions read.
