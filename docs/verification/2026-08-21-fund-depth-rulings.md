# Fund depth — the design rulings, the CEF probe, and the built leg (2026-08-21)

The fund-depth slice's design rulings, the live probe that re-scoped its CEF
leg, and the build inventory.
The slice was the Portfolio completion block's last bullet
(`BUILD.md` §What remains), gated behind its own design ruling since the
2026-08-15 conformance walk recorded R27's open item.

## The rulings

Four user rulings, in the order they were made:

1. **The flat-driver fund target form is the settled design, not a stopgap.**
   The shipped v2-over-composite form — the driver `spot × composite earnings
   yield` held flat across scenarios, all scenario width from the multiple axis
   — is the priced-fund scenario-target design, closing the conformance walk's
   R27 open item ("scenario-differentiated priced-fund formula, undesigned").
   No honest fund-level consensus exists on the current surface; the named
   conservative bias (no earnings-growth leg) is exactly what the outcome
   scorecard's signed base-case error measures.
   A scenario-differentiated formula returns only on realized-outcome evidence
   — the same evidence-first rule that retired sector-aware grade
   normalization (2026-08-13).
   The candidate differentiations weighed and set aside: an N-PORT
   constituent-consensus look-through driver (couples the formula to the
   deferred N-PORT leg; ~60-day-stale weights), and a derived sector-growth
   leg from `historical-sector-pe` + `historical-sector-performance` (an
   equal-weight averageChange series against a cap-aggregate P/E — the same
   unsound-aggregate shape the composite already rejects).
2. **N-PORT look-through stays deferred.**
   It remains the named optional enhancement — concentration enrichment,
   never floor-bearing — owned by no slice.
3. **The CEF price-vs-NAV read is prompt evidence + card only.**
   Never a score, rule, or ledger series; no NAV history is on-plan, so a
   discount-vs-own-history percentile is not honestly computable and a
   drafted score band would be arbitrary.
4. **The CEF leg is the detection + gap-honest seam** (re-scoped after the
   probe below): profile-based detection with the "closed-end fund" card
   label, the NAV read rendering when both usable legs exist — a positive
   market quote and a positive NAV — and recording a named gap otherwise,
   its text naming the missing leg; on today's surface the NAV leg is always
   missing for a real CEF.

## The probe

`fmp_cef_probe` (`src-tauri/src/fmp.rs`, `#[ignore]`d, 8 one-shot calls; run
2026-08-21 against the live paid-plan API): PDI (bond CEF), GAB and BST
(equity CEFs), SPY as the open-end control, each against `/profile` and
`/etf/info`.

Findings:

- **`etf/info` serves every probed CEF an empty body (`[]`)** while serving
  SPY its full record — a CEF gets no NAV, expense ratio, asset class, or
  weightings from the fund surface.
  No other on-plan endpoint serves CEF NAV (`references/fmp-api.md` swept),
  so the ruled premium/discount render has no live data path today.
- **The profile carries the detection signal**: `isFund: true` (confirmed on
  PDI; FMP's flag covers open-end mutual funds too, so it is necessary, not
  sufficient) and a closed-end fragment verbatim in all three descriptions
  ("closed-end mutual fund", "closed ended equity mutual fund", "closed-end
  equity fund").
- Consequence for the pre-leg behavior: a real CEF reaching the fund path had
  been abstaining `insufficient-evidence` ("fund metadata unavailable") on
  every run — the docs' "routes as a generic fund" understated the degrade.

## What shipped

- **Detection** — `fund::is_closed_end`: the profile's `isFund` flag AND a
  closed-end description fragment, both required, so a missing or ambiguous
  profile never guesses a CEF.
  The adapter's `fetch_fund_data` gained the one-per-fund `/profile` leg
  (fail-soft; a failed read records the "closed-end detection cannot run"
  gap), full-pass only — the quick check never re-runs detection.
- **Classification** — `is_cef` on `FundClassification` as a structure marker
  orthogonal to the strategy class (a bond CEF still routes bond); labels
  carry "(closed-end)", and the empty-`etf/info` CEF takes the `role_risk_only`
  readout labeled **"closed-end fund"** instead of abstaining every run.
- **The read** — `fund::nav_premium_read` computes market price ÷ NAV − 1 from
  the market price only (the NAV-fallback spot would fabricate an exact 0%
  premium precisely when no quote exists — this also fixed the priced path's
  existing spot-based computation).
  Consumption per ruling 3: a shared prompt line in the role-risk
  interpretation, action, and priced-fund interpretation prompts plus a card
  line (`RoleRiskVerdict.is_cef` / `nav_premium`, serde-defaulted on pre-leg
  rows), all gated on the closed-end marker; absence is the named
  price-vs-NAV gap in the evidence-gap manifest.
- **Delta gating** — the 6g input delta's "NAV premium" metric row is
  CEF-gated, so an open-end ETF's transient premium flicker never seeds an
  input-delta row.

The metis review round caught one docs/code mismatch the first pass shipped:
the quick check's live provider routed through the same `fetch_fund_data` the
profile leg had joined, so the sweep fetched an unconsumed profile per fund
while three docs said the read was full-pass only.
The fix aligned code to the docs' stated intent — the sweep now calls a
`fetch_fund_refresh_data` variant with no profile leg, request-count-pinned by
test — and added two of the review's suggested pins: a pre-CEF-row
`RoleRiskVerdict` deserialization test and the priced-branch prompt's
closed-end arm.

A third round (external Codex review) landed four findings, all verified
against the code and accepted:

1. A malformed 200 profile body lost the promised detection gap —
   `suite_get_shaped` folds malformed into `Ok`, so only transport / HTTP
   gaps reached the outer gap push; the shaper now pushes the gap itself
   (the weightings-leg pattern), test-pinned.
2. The premium label read `>= 0` as "premium", so an exact or rounded zero
   rendered "+0.0% premium" / "-0.0% discount"; prompt and card now label by
   the rendered tenth of a percent, with a rendered zero reading **at par**.
   And the CEF gap said "no NAV" even when the actual absence was the market
   quote (the spot floor falls back to the NAV); the gap now names which leg
   is missing.
3. The role-risk audit metrics omitted `nav_premium`, so a CEF premium move
   could never seed the input-delta row the logic-flow doc claims; the read
   now joins the branch's computed surface — populated on the closed-end form
   only, so a non-CEF fund's transient premium still never rides the verdict,
   prompts, or audit.
4. Provenance staleness: the persisted fund-source label and a dossier
   comment omitted / denied the new profile call, and one logic-flow line
   still said the NAV premium never reaches the priced-fund prompt — all
   corrected.

A fourth round (Codex) confirmed round 3's finding 3 fixed and landed three
follow-ons, all verified and accepted:

1. The malformed-gap fix covered only unreadable bodies — a served empty, a
   fieldless object, and a flag-drifted `isFund` still read silently.
   The shaper now leaves a detection trail for every answer that stops the
   detection running: empty, unreadable, flagless, and the fund-flagged
   profile with no description to screen; the one silent answer is a
   definitive `isFund: false` (nothing to detect), all five shapes
   test-pinned.
2. The boundary fix diverged between the arms — Rust rounds the exact
   negative half away from zero while `Math.round` takes it toward +∞ — and
   the gap cause keyed on quote *presence* while the parser passes a zero
   price through.
   The Vue helper now rounds half-away-from-zero to match, and the cause
   keys on usability (`NAV > 0`, price > 0), both test-pinned.
3. Two more stale "funds get no profile call" comments and the logic-flow's
   role-risk readout list (missing the closed-end marker and the read) —
   corrected.

A fifth round (Codex) confirmed the round-4 corrections and landed two low
gaps, both accepted: the logic-flow's two CEF gap lines still named only the
absent-NAV cause (now stated as either usable leg, the text naming which),
and the Vue rounding correction had no frontend regression test — the
PortfolioView spec now pins the rendered CEF row (signed premium/discount,
the −0.05% away-from-zero boundary, at-par, the absent-row gap and non-CEF
cases).
That spec's existence also corrected this record's implementation-round
belief that no PortfolioView spec existed, and `CLAUDE.md`'s stale spec list
(PortfolioView and ConfirmDialog were missing).

A sixth round (Codex) landed two low consistency gaps, both accepted: this
record's own ruling-4 sentence still stated the NAV-exists form (now the
both-usable-legs form the code enforces), and two further role-risk spec
fixtures omitted the required `is_cef` / `nav_premium` fields (type-stripped
specs pass impossible wire shapes silently — all five role-risk fixtures now
carry them).

Gates at completion: `cargo test` 1141/0, `cargo clippy --all-targets
--all-features` clean, `npm run build` clean, `npm test` 46 + 239 pass.

## Accepted residue and watch items

- **Schwab typing is unverified**: whether Schwab serves a held CEF as
  `COLLECTIVE_INVESTMENT` (→ the fund path, where this leg lives) or `EQUITY`
  (→ the stock path, which would floor-abstain it without reaching detection)
  cannot be verified without holding one — a big-run watch-set candidate, not
  a slice defect.
- **The priced-branch card carries no NAV line** — pricing requires
  weightings, which today's surface never serves for a CEF, so only the
  prompt line and the "(closed-end)" label ship on that branch; the role-risk
  branch (the reachable one) carries the full card seam.
- The CEF research-agenda topic (discount and distribution coverage) rides
  the research-loop slice, as designed.
