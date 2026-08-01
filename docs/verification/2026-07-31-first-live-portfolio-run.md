# First live Portfolio Analysis run — 2026-07-31

*The evidence record for the first live end-to-end Portfolio Analysis run — the M5-gated spine shakedown
(live Schwab + live FMP/SEC/Stooq/FRED + the pinned local daemon, research stubbed).
This file holds the measurements, per-finding evidence, and environment;
follow-up work it motivates is listed at the end as candidates, not decisions — sequencing stays with the build queue.
The run itself is persisted in the dev store (`portfolio_runs`, run `3b21ae85-bac7-41fe-9ced-1b2bb1f5571e`) and is the raw dataset behind every claim here.*

## Environment

- **App:** dev build (`target/debug/market-signal`, Tauri dev harness), fresh `dev/` data dir created at launch;
  the production corpus (27 reports, vector memory, baseline snapshots) imported via the portability archive so the house view was real and current
  (latest report 2026-07-31 — same-day fresh).
  Incidental verification: this was the **first live whole-corpus portability round-trip on real data** (unencrypted — no passphrase used),
  and it arrived intact end-to-end; the *encrypted* round-trip remains the open optional check.
  `portfolio_runs` and `job_runs` started empty, so this was a true first run (every position tagged `new`, no prior verdicts, no diff).
- **Daemon:** pinned Ollama v0.32.5 (`~/ollama/v0.32.5/ollama serve`), `OLLAMA_FLASH_ATTENTION=1`,
  plus a deliberate test-session deviation from the documented start command: `OLLAMA_KEEP_ALIVE=2h`,
  so the pre-run warm-up survived the setup phase (the stock 5-minute idle unload would have silently discarded it).
- **Roster:** reasoner `qwen3.5:122b-a10b` (fills the fast slot too — fast left blank), embedder `qwen3-embedding:4b` (present for the gate, never called — nothing embeds in this slice).
- **Settings path exercised for real:** Local-analysis-models section filled + saved (presence warning cleared on save),
  manual Test Connection returned *daemon reachable — all rostered models available*, Schwab freshly reconnected (client id + secret + browser OAuth).
- **Instrumentation:** temporary per-call logging (request shape, wall-clock, Ollama done-chunk eval stats, full prompt/content/thinking dumps)
  in `local_model.rs` / `portfolio/pipeline.rs` / `portfolio/job.rs`, working-tree only, reverted after the run.
  The raw session log (~1.65 MB, 15.6 K lines — contains the full holdings book, keep local) lived in the session scratchpad.

## Scope

Tested: the live spine — gate, Schwab pull + normalization, per-holding data gather, the deterministic engine on real data,
grammar-constrained interpretation on the live 122B, the run lifecycle, persistence, and the Portfolio page render.
Deliberately absent (designed, not built, or stubbed): web research (the stub note rode every distill call),
the 7b construction stage (the roll-up is deterministic counts), all depth slices (thesis ledger, quick check, selective re-analysis,
refresh lane, pre-profit overlay, outcome learning), and the configurable investor profile (fixture preset).
Verdict shallowness relative to the finished feature is therefore expected and not a finding.

## Run summary

- **Book:** 47 positions after normalization, cash $83, account total $233,613; top position 24 % of the account.
- **Outcome:** `successful`, 2 h 30 m 57 s wall-clock (20:08:37 → 22:39:33 UTC), zero failed requests surfaced as run failures, no cancellation.
- **Dispositions:** 44 priced (16 C / 10 D / 18 F — zero A/B), 1 `role_risk_only` (ARKF, "ex-US equity fund"), 2 `insufficient-evidence` (NIO, RKT — the evidence floor, no model calls), 0 not-rated.
- **Actions:** 36 sell-all, 6 trim, 3 hold. **Hurdle states:** 35 fails / 9 indeterminate / 0 clears.
- **Model calls:** 44 distill (44.7 min total, avg 61 s) + 45 interpret (100.0 min total, avg 133 s, max 238 s) — 96 % of wall-clock was model time;
  ~1.15 M chars of thinking across 89 dumps.
  45/45 grammar-constrained calls returned schema-valid JSON on the first attempt; the feasible-set defense-in-depth never tripped.

## Machinery verification (all green)

- **Local gate + Test Connection** behaved to contract: blocked before config, presence warning cleared on save, probe green, run gate passed.
- **Live Schwab:** OAuth round-trip, holdings pull, book-level netting, and per-symbol option chains all worked on the first attempt;
  option-signal fields populated where chains existed.
- **All four disposition branches executed live**, including both fund routes —
  the ≥ 70 %-US guard discriminated correctly between sibling funds (ARKK priced, ARKF `role_risk_only`),
  priced funds carry `low_confidence_grade` + the class label, stocks don't.
- **Evidence floor:** NIO and RKT abstained honestly ("only 1 of 3 letter sub-scores computable (need 2)") in ~3 s each, zero model calls.
- **Fail-soft data posture:** every degradation (Stooq, SEC 404s, FMP field gaps) landed as a typed gap in the audit *and* the prompt; none failed the run.
  The rate-anchor hard-fail rule was armed but FRED delivered (both prints + history).
- **Audit records complete** on all 47: `prompt_version=portfolio-v2`, deduped `model_ids`, sources, degraded inputs, and `target_meta` (`targets-v2`) on priced holdings.
- **Persistence + UI:** one run row, `job_runs` records `successful`, the runs-history sidebar shows "Full book · 47 holdings · RATED 44",
  cards render the full priced surface (sub-scores, conviction meter, both targets with bands, outlook, options signal, action + sizing,
  financial-analysis prose, what-changed strip with the `POSITION: NEW` badge), and the footer links the run log.
- **Timeout margin:** longest call 238 s against the 600 s backstop — the cold-start question is retired
  (first weight load measured 13.3 s on this session's warm-up; the model stayed 100 % GPU-resident, 87 GB, ~30 % system memory free throughout).

## Findings

### F1 — Flat-target syndrome (headline; calibration, not a code defect)

The dominant result — 35 of 44 priced holdings reading `dead_money=fails` and the resulting sell-all avalanche — traces to
**scenario targets that sit essentially at spot**, not to model behavior or a broken hurdle.
Evidence (base vs spot): PGNY bear=base=bull=31.13 = spot exactly; V base 366.13 = spot to the cent; UBER base 70.36 = spot;
NFLX base 71.71 vs 71.77; AAPL base 308.91 vs 309.20; META base 556.71 vs 556.00; TSLA base 311.20 vs 309.97 (bull collapsed onto base).
Three documented mechanisms compounded:

1. **The anchor join never engaged.** Stooq failed run-wide (F2), so `target_meta` shows `rate_anchored=false, anchor_observations=0, current_multiple_carry=true`
   on every inspected holding — the multiple fell to the current-multiple carry, exactly as documented, but for the *whole book*.
2. **The consensus driver ≈ trailing EPS.** The selected consensus row's EPS matched TTM almost exactly
   (NFLX: driver 2.637 vs TTM 2.639 implied by spot ÷ P/E), so `base = fwd EPS × current multiple ≈ spot` — no growth reaches the driver.
   Whether the nearest-period selection *should* skip the mostly-reported current fiscal year is the calibration question this run surfaces.
3. **Near-period consensus bands are tight**, so the bear/bull dispersion is a few percent —
   and a bull case of +1–4 % total return against a DGS2-anchored, tier-scaled hurdle (7.2 % low-tier, 9.2 % medium observed live)
   means *even the bull leg misses* → `fails` by the three-state definition.

**The hurdle logic itself verified exactly right:** every observed `indeterminate` is a case whose bull leg cleared the hurdle
(DIA bull +10.0 %, NKE +32 %, UBER +13.7 %) while the bear leg missed — the three-state semantics behaved to spec on live data.

*Correction note for the record:* the in-flight diagnosis during the run first read NFLX's targets as "~ −94 %, mis-scaled EPS"
by comparing against a remembered pre-split price level instead of the in-data spot ($71.77 post-split);
the FMP consensus shape is internally consistent on the post-split share basis and carries no scale error.
The corrected analysis above is what the persisted data supports.

### F2 — Stooq deep history failed for every holding after the first

"Stooq body did not start with the daily-bars CSV header" on all 43 requests after TSLA —
the signature of Stooq's daily-hits limit tripping (an HTML notice body), not a parse bug.
Consequence: the v2 spread-anchored multiple path was starved for the whole run (see F1.1), silently but *recorded* — every audit carries the gap.
The fail-soft contract worked; the calibration consequence is that one throttled keyless source can flatten the entire target surface.
Candidate mitigations for a future slice: per-run Stooq budget/caching, request pacing, an alternate deep-history source, or letting FMP's own history serve the anchor join when Stooq is throttled.

### F3 — `think: false` never reaches the wire

`ChatWire` skips serializing `think` when false (`skip_serializing_if = "is_false"`), so the distill stage's intended non-thinking call
rides Qwen's thinking-on default: 27–121 s and up to 4,260 generated tokens per call to condense a one-sentence stub — **44.7 minutes of the run**.
The #14645 verification unlocked `think:false` + `format` on this pinned version precisely so distillation could run non-thinking;
the adapter just never says it.
Belongs in the options-wiring slice (alongside `num_ctx`, per-mode sampling, `keep_alive`): send `think` explicitly for non-thinking stages.

### F4 — Grade bands compress the whole book to C/D/F

Zero A/B letters across 44 priced holdings; META, MSFT, V, AAPL, NVDA, NFLX all C or worse
(NFLX: quality 49 / valuation 27 / risk 37 → F on a 24 % net-margin business).
Three contributors, in fix order:
statement-field gaps depress axes (F5) — tune data completeness before touching constants;
the valuation band reads large-cap-growth multiples as expensive (a P/E of 27 scoring 27/100);
and sub-score dispersion at the extremes looks unstable (RMBS: quality 100 / valuation 4 / risk 0).
**Relative ordering carries real signal** (speculative/distressed names sit below the franchises; the floor abstained rather than junk-grading),
so this is exactly the shadow-tuning the design deferred ("calibratable, to be shadow-tuned against live runs") — this run is the first calibration dataset,
with every sub-score, metric, and gap persisted in the audits.
One caveat on scope: the sub-score *formulas* themselves were not audited against their spec in this run
(the target arithmetic and hurdle semantics were verified line-by-line; the normalization bands were only judged by their outputs) —
certify them against this run's persisted metrics as the first step of the band-tune slice.

### F5 — Per-symbol data-adapter gaps

FMP left revenue-growth and debt/equity gaps on some large caps (NFLX both), and SEC EDGAR 404'd on others (QQQ prose names it);
each gap lands honestly but pulls the letter down through F4.
Worth a targeted pass over the per-company fetch (statement-period selection, fallback fields) before band tuning.

### F6 — Model judgment reads well within its inputs

The model never invented a number, echoed engine figures faithfully, and its prose *names its own data gaps*
(QQQ's financial summary explicitly cites the SEC 404, the Stooq gap, and the imputed quality-50 low-confidence letter).
On the 9 indeterminate-hurdle holdings it genuinely differentiated (3 hold / 3 trim / 3 sell-all),
and it overrode the failed-hurdle exit tilt with trims on exactly the three largest positions (VGT, QQQ, SOXQ) — position-size-aware restraint.
The thinking dumps confirm this was deliberation, not luck: VGT's reasoning reads
*"Trim keeps exposure but reduces concentration in a low-grade vehicle"* and
*"avoiding full concentration in a failing fund, but keeping some beta to the constructively leaning sector"* —
portfolio-context judgment no engine stage computed — and the house view is genuinely engaged
(VGT's deliberation references it 11 times; NVDA's cites the report by title).
The honest limit: with research stubbed and a flat target surface, ~80 % of actions were mechanical tilt-following,
so the interpret stage is *demonstrably capable but under-supplied* — its marginal value scales with the two inputs the next slices add
(live research, targets with signal).
Two calibration notes: `fails` → sell-all was otherwise near-mechanical (32 of 35),
and "sell-all at low conviction" appeared twice (SPMO, SCHA) — an odd pairing worth a prompt nudge when the target surface is fixed
(with flat targets the model had little to push back with).

### F7 — Ollama done-chunk stats exclude thinking

On v0.32.5, `eval_count`/`eval_duration` cover only the post-thinking content phase;
the thinking phase rides inside `total_duration` unaccounted (NFLX interpret: 103 s elapsed vs 6.6 s "eval" for 293 "tokens" — the missing ~90 s was ~4 K thinking tokens at ~45 tok/s).
Any future throughput instrumentation must compute effective tok/s from accumulated chars ÷ elapsed, not the reported eval fields.

### F8 — The tracker is silent through interpretation

Interpret calls stream `StreamRole::Silent`, which emits nothing — a 2–4-minute quiet stretch per holding with only the step row and a "Local" request row open.
The `pipeline.rs` comment claiming the reasoning reaches the tracker's thinking channel is wrong against the enum (`Silent => {}`).
Candidate UX for the options slice: a posture-tagged or per-holding thinking channel; at minimum, fix the comment.

### F9 — Result-review UI findings (user review of the rendered run)

Three defects surfaced reviewing the cards, none engine-side:

- **The roll-up stat strip's hairlines break on wrap.** `.strip.keyfig` draws interior dividers via per-cell
  `border-left`/`border-top` + `-1px` margins clipped by the container — an idiom that only closes up when the
  `auto-fit` grid stays on one row; a wrapped row (observed: TOP POSITION dropping to row 2) leaves row-1 cells
  above empty grid space with no bottom hairline and boxes the first cell. Needs the gap-background (or
  equivalent) hairline pattern that survives wrapping.
- **The verdict card never shows the current price.** Targets, bands, and P/L render, but the reference point
  they're measured against appears only inside model prose ("vs current ~200"); `current_price` is rendered
  only in the standalone holdings-pull table. The position row is already joined for the card header, so this
  is a small addition beside the targets.
- **"Trim TO 0–0 %" on sub-1 % positions is a formatting artifact, not an engine error.** ARKK's persisted band
  is 0.27–0.48 % of a 0.68 % position — correct trim math (0.4×/0.7× current weight) — but the weight formatter
  is `toFixed(0)`, which rounds any sub-half-percent band to "0–0 %" and makes a trim read as a sell-all.
  Adaptive precision (one decimal below ~2 %) fixes it; an engine-side change is not needed.
- **The card shows no invested-capital facts.** Total cost basis (the netted book-level dollar total) and the
  average per-share cost (total ÷ quantity — the across-orders average) never render, so an unrealized figure
  like "+$54,414 (2925.9 %)" floats without its base. Both derive directly from the persisted position row;
  they belong beside the current price in the card's position block.
- **The footer's LAST RUN readout is not job-scoped.** `jobs::job_status` queries `job_runs` with no
  `job_type` filter (`WHERE state = ?1 ORDER BY id DESC`), so after the portfolio run the report footer's
  "LAST RUN Jul 31 03:39 PM" displays the *portfolio* job's finish under chrome whose "Generate now" button
  triggers the *report* — invisible before the suite existed (one job type), surfaced by the first local run
  in a fresh store. Scope the readout to the report job (and consider relabeling the button "Generate report"
  on suite pages, where a Portfolio "Run analysis" button coexists).
- **"Hold TO 18–22 %" reads as movement.** The band itself is correct engine semantics — every action carries a
  target-weight band, and Hold's is 0.9×–1.1× current weight (19.9 % → 18–22 %, est. shares 0) — but the
  shared "TO" preposition implies an adjustment; Hold should render "maintain 18–22 %" (or "≈ 20 %").

### F10 — Operational notes (dev-app driving)

`tell application "Market Signal"` resolves through Launch Services to the *release bundle* and relaunched the production app mid-session
(two instances, same display name — drive the dev app by process name `market-signal` only).
The relaunched production instance hit the documented Keychain-ACL first-paint block (stacked prompts, blank webview) and was quit.
Keychain prompts on the fresh dev binary behaved as documented (Always Allow clears them per binary).

## Follow-up candidates (not sequenced here)

1. **Adapter options-wiring slice** (already queued, scope now sharper): explicit `think` per stage, `num_ctx` per stage, per-mode sampling, `keep_alive` residency — F3/F8 fold in.
2. **Target-function calibration**: consensus-period selection (skip the mostly-reported current year?), dispersion floor or multi-period blend, and the hurdle interplay — F1; re-run against this book to diff.
3. **Stooq resilience**: pacing/caching/alternate history source so one throttle can't flatten the anchor surface — F2.
4. **Per-company data completeness**: FMP statement fallbacks + SEC coverage — F5, prerequisite to
5. **Grade-band shadow-tune** against this run's persisted audits — F4 — opening by certifying the sub-score formulas against spec (the unaudited corner of the math).
6. **Interpretation-prompt adjustments** (after 2/4/5 — tuning prose against a broken number surface is tuning to noise), four concrete edits — F6:
   surface **target provenance** in the prompt (the audit knows `rate_anchored=false` / `current_multiple_carry`; the model never sees it —
   the single change that would have broken this run's sell-all cascade);
   soften the dead-money **tilt into a weighed input** ("weigh it against target dispersion and data quality") rather than an instruction;
   **define conviction** (confidence in the overall read, consistent with the action's decisiveness — fixes the sell-all-at-low-conviction pairings);
   and **scope the house view** to horizon reads / market-setup context so a defensive report can't amplify per-holding exit bias.
7. **Run-level data-health roll-up**: per-holding fail-soft was honest but no signal aggregated "43 of 44 anchor windows empty" at run level —
   a degraded run that looks clean produces confidently wrong prescriptions; surface a data-health line on the run (roll-up, card header, or warning).
8. **Portfolio-page polish micro-slice** — F9's display items: wrap-safe stat-strip hairlines, a proper position block on the verdict card
   (current price, total cost basis, average per-share cost), adaptive weight-band precision, and Hold's "maintain" phrasing.
   Small, display-only, independent of the calibration work.
9. **Section-scoped footer + report nav entry (user-settled design, 2026-07-31)** — F9's footer item, resolved as three parts:
   a **"Latest Market Report" entry joins the sidebar nav** (today the Portfolio page is a navigation dead-end back to the report —
   the shared sidebar swaps to runs-history, so the Recent Reports list is gone and no nav item reaches the report view);
   **"Generate now" renders only when the report view is the main window** (each section keeps its own trigger — Run analysis on Portfolio, the TO buttons later);
   and **the footer's LAST RUN readout scopes to the active section's job** (`job_status` filtered by `job_type`: report vs portfolio vs TO — today it shows the latest row of any type).
   `docs/interface.md §Main Layout` is the canonical home to amend when this lands.

## Verdict on the shakedown

The spine holds: every mechanical contract — gate, live adapters, engine branches, schema-constrained interpretation, lifecycle, persistence, UI — executed correctly on the first live attempt, and every failure mode that appeared was a *calibration or upstream-data* finding, recorded honestly by the system's own audit trail.
The local-model path is production-viable on this hardware (≈ 3.2 min per priced holding, 96 % model time, stable memory);
the product output (letters and actions) is not yet trustworthy, for the reasons in F1/F4, and says so in its own prose more often than not.
