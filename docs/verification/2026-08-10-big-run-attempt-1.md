# Big confirmation run — attempt 1 (2026-08-10)

*The evidence record for the first attempt at the single big confirmation run
(`verification/big-run-watch-set.md` is the checklist it reads against).
The attempt **failed at Step 7b** after 2 h 46 m and persisted nothing, so most of the watch set is
still unread and the run has to be repeated.
This file holds the failure analysis, the findings read off the run's reasoning panes, the small set of
items the attempt did confirm, and the fix candidates they motivate — candidates, not decisions;
sequencing stays with the build queue.
Findings 2–6 come from reviewing the live reasoning and tracker panes during the run, and each was
verified against the code before being recorded here.
Because no run row persisted, the per-holding evidence is gone: every claim here rests on the
`job_runs` failure detail, the run tracker captures, and the code.*

## Environment

- **App:** dev build (`target/debug/market-signal`, Tauri dev harness) against the existing `dev/`
  data dir, which held one prior run (`3b21ae85`, 2026-07-31, the v2 calibration baseline).
  That run was kept deliberately: it is the only run on disk carrying a pre-`grade-v2.1` stamp, so it is
  the sole input that exercises the band-recalibration continuity path.
  Its decode against current types was verified before the run — 44 priced / 1 role-risk / 2
  insufficient, matching its own record.
- **Daemon:** pinned Ollama v0.32.5 (`~/ollama/v0.32.5/ollama serve`) with `OLLAMA_KEEP_ALIVE=2h`.
- **Roster:** reasoner `qwen3.5:122b-a10b`, embedder `qwen3-embedding:4b`, fast slot blank.
- **Gates before the run:** 1018 lib + 32 integration tests, clippy clean, `npm run build` clean,
  46 node + 225 vitest.
  One test-only fix was needed to get there and is recorded under Residue.
- **Instrumentation:** none added to the app.
  The run executed the shipped binary.
  External capture only: the Tauri stdout log, and a 90-second screenshot loop over the run-tracker
  window (117 captures) added because per-request rows and the reasoning panes are rendered live and
  never persisted.

## Run summary

- **Outcome:** `failed` at 04:47:13 UTC, started 02:00:43 UTC — **2 h 46 m 30 s**.
- **Failure:** `portfolio construction jointly infeasible after the named-violation re-run`,
  with 44 violations across 38 distinct symbols.
- **Persisted:** nothing.
  `portfolio_runs` still holds only the 2026-07-31 run; `price_bars`, `portfolio_outcome_episodes`,
  `portfolio_quick_checks` and `holdings_pulls` are all empty.
  Checkpoint/resume is unbuilt (`BUILD.md` §Owned by no slice), so the entire per-holding pass was discarded.
- **Book size for this run is unrecorded**, since the holdings snapshot persists at Step 8.
  38 distinct symbols appear in the violation list.
- **Model calls (from the Ollama server log, which survived):** 95 `POST /api/chat`, 19:01:25 → 21:47:13.
  Total model time **2 h 44 m**, against 2 h 46 m 30 s of wall-clock — **98 % of the run was model time**,
  up from 96 % on 2026-07-31.
  Mean 103 s, median 56 s, max 492 s.
  The distribution is cleanly bimodal — 48 calls under a minute and 40 between two and four minutes —
  consistent with one distill plus one interpret per holding, so roughly 93 per-holding calls plus the
  two construction attempts.

## Finding 1 — Step 7b discards the whole run over unattributable action changes

### What the stage does

Steps 1–6 analyse each holding in isolation and produce an intrinsic verdict plus a **standalone
lean** — the action for that position considered alone.
Step 7a computes the deterministic whole-book picture.
**Step 7b is the single model call that reconciles the two**, emitting each holding's final action and
target-weight range, and it is the sole action author for `role_risk_only` holdings.

Because 7b may overrule a standalone lean, it must attribute what it changed.
An action that moved from its baseline carries a `changed_attribution` of `moved-intrinsic` or
`moved-context`, and a context claim must map to a real Step-7a aggregate rather than being asserted.
`docs/portfolio-workflow.md` §Step 7b splits the checks deliberately: engine-bound checks annotate and
never enforce, while self-coherence stays enforced and *"persisting incoherence fails the run like any
hard model failure"*.
The fail-hard is therefore designed behaviour, not a defect in the validator.

### The violation census

| Class | n | Detail |
|---|---|---|
| A — action moved, no `changed_attribution` | 26 | see transition table below |
| B — context cause maps to no real aggregate | 13 | 11 × `cash-freed`, 2 × `became-oversized` |
| C — final action departs the lean, no `divergence_cause` | 3 | AMZN, ARKQ, V |
| D — `moved-context` without a `changed_cause` | 2 | ALAB, NVDA |

Class A transitions:

| Transition | n |
|---|---|
| `sell-all` → `hold` | 15 |
| `sell-all` → `trim` | 5 |
| `trim` → `hold` | 3 |
| `trim` → `sell-all` | 1 |
| `hold` → `add` | 1 |
| `sell-all` → `add` | 1 |

**25 of the 26 unattributed moves soften an exit**, and only one moves down the ladder.

The baseline they moved *from* is the **prior run's** action, not this run's engine read.
`construction.rs:1345` binds it as `if let Some(prior) = x.row.prior_action`, and raises
`WhatChangedMissing` whenever this run's final action differs from it without an attribution.
That matters for the causal story: 21 of the 26 are moves off `sell-all`, and the prior run
(2026-07-31) recorded **36 sell-alls out of 44 priced holdings** — the documented flat-target cascade.
The attribution burden on this run was therefore set by the previous run's degenerate output.
A bad run does not merely produce a bad record; it taxes the next run that disagrees with it.

### Root cause — output-budget exhaustion, not vocabulary ignorance

The tracker capture at 21:46:51, twenty-two seconds before the failure, holds the model's reasoning
during the re-run.
It states the constraint directly:

> *"Given length constraints, I will provide a representative corrected set for key drivers and fix
> specific errors noted in validation text... Since I cannot exceed context window too much, I'll
> prioritize accuracy on the errored fields while maintaining structure for others."*

> *"Token limit is tight if I output 60+ objects with full detail.
> I'll compact representation where possible but ensure validity constraints are met in every object."*

The model was not confused about the vocabulary.
In the same reasoning block it forms a correct attribution unprompted:
`NVDA: Trim. Divergence Cause = overlap-emerged / became-oversized. Attr = moved-context`.
It dropped attribution fields on the holdings it compacted, and those are exactly what the validator enforces.

The mechanism is a shared budget with nothing reserved for output.
`NUM_CTX_INTERPRET` is 131,072 (`portfolio/pipeline.rs:2658`) and covers input *and* output together.
**`num_predict` is set nowhere in the crate** — zero occurrences under `src-tauri/src/`.
So the room left to write the plan is whatever the prompt did not consume, and the prompt carries every
holding's digest plus the Step-7a aggregates.

Two properties make this worse as the book grows.
The output requirement scales with position count, because every holding needs an object.
The *attribution* requirement scales with how many final actions differ from the prior run's, which the
previous run's 36 sell-alls drove to a large fraction of the book.

### Why the re-run could not recover it

The single named-violation re-run (`portfolio/job.rs:1193`) resends the construction input with the
violation list appended, and asks for the corrected full plan.
That makes the prompt **larger** than the attempt it is rescuing, which leaves **less** output room, while
the number of objects to emit is unchanged.
The recovery path is therefore more budget-constrained than the failure it exists to repair.
The model's own reasoning shows it resolving that squeeze by compacting further.

### Why `cash-freed` could not validate

`cash-freed` validates only when the plan actually raises proceeds and the move is up-ladder
(`portfolio/construction.rs:1295`): `sells > DOLLAR_EPS && rung_index(action) > rung_index(baseline)`.
The model softened the exits, so the plan sold little or nothing, so no cash was freed.
Eleven holdings cited freed cash regardless.
This is a second-order consequence of the same overriding, not an independent fault.

It also leaves an open question worth ruling.
The most natural justification for softening an engine exit is unavailable by construction whenever the
model softens exits across the book, and the remaining vocabulary
(`became-oversized`, `overlap-emerged`) describes reasons to *reduce* a position, not to keep one.
Whether the vocabulary needs a term for declining an engine exit — and whether a `changed_attribution`
should be demanded at all when the baseline is an engine stand-in that `portfolio-v7` entitles the model
to depart from — is a design question this run surfaces but does not answer.

### Severity

The validator is correct and its messages are precise.
The severity is in the blast radius.
A single unsatisfiable whole-book call discards a completed 47-holding pass, its ledgers, its overlays,
its audits and its outcome episodes, with no resume path.
Every stacked watch item the pass would have answered is lost with it.

### Fix candidates

Listed as candidates for a ruling round, in the order that recovers the most value per unit of risk.

1. **Persist a run whose construction fails.**
   This is the highest-value change and the smallest.
   `PortfolioRollUp.construction` and `.aggregates` are already `Option`, so the persisted shape
   already admits a run with no constructed book — the abort happens before Step 8, not because the
   record cannot represent it.
   The candidate is to treat persisting incoherence as a terminal *construction* failure rather than a
   terminal *run* failure: persist the verdicts with an unconstructed-book marker and surface the run
   as degraded.
   This is a spec change as well as a code change, because §Step 7b currently ties the outcome to a
   hard model failure, so it needs a ruling before it is built.
2. **Send the re-run only what it must repair.**
   Re-authoring the full plan is what makes the recovery path more constrained than the original attempt.
   Asking for corrected objects only for the violating symbols shrinks the required output sharply, and
   shrinks it most precisely when the violation list is longest.
3. **Compress the construction digest.**
   This is the sanctioned response to context pressure under the standing constraint —
   compress digests, never raise `num_ctx` — and it is the input-side half of the same squeeze.
   The B12 instrumentation already measures prompt fill and sent size per call, so the digest's
   contribution is measurable once a run persists.
4. **Reserve an output budget explicitly.**
   With no `num_predict`, nothing distinguishes "the prompt is large" from "there is no room to answer".
   An explicit reservation would turn a silent compaction into a legible limit.
5. **Rule the attribution questions in §Why `cash-freed` could not validate.**
   Smaller than the above and dependent on them.

Chunking construction across the book is **not** a candidate.
Joint feasibility is whole-book by definition, and splitting it would break the property the stage exists
to enforce.

### Cost of the failed stage

The Ollama server log survived the run and dates every model call.
The two construction attempts are the run's two slowest calls by a wide margin: **7 m 02 s** for the first
and **8 m 12 s** for the named-violation re-run, completing at 21:39:01 and 21:47:13.
The re-run took *longer* than the attempt it was rescuing, consistent with its larger prompt.
**15 m 14 s went into construction**, and the run failed on the second call's return.
Both calls returned HTTP 200 — the model answered each time, and the answer was incoherent, not absent.

### Unconfirmed

Whether **this** run's engine targets were themselves degenerate is **not established**, and the one
sample available argues against simply assuming a repeat of 2026-07-31.
A reasoning pane for SBUX shows engine targets that are steeply bearish rather than flat —
12-month bear 59.63 / base 69.21 / bull 104.46 against a spot near 104.65, with the capital-efficiency
read failing.
That is a base **34 % below spot**, which is a different shape from the flat-target syndrome where the
base collapsed onto spot.
One holding is not a distribution, and the evidence that would settle it — `target_meta` provenance and
the hurdle distribution across the book — was in the run that did not persist.
The repeat attempt should read it before anything else.

## Finding 2 — the response schema is enforced but never declared, and the model re-derives it every call

The interpretation call is grammar-constrained: `pipeline.rs:2719` sets
`req.format_schema = Some(interpretation_schema())`, which Ollama enforces as a native `format`.
The prompt, however, closes with only *"Respond only with the required JSON object"*
(`pipeline.rs:1387`) and never states what that object is.
The model is therefore given a hard constraint it cannot see, described by a sentence that does not
describe it.

The reasoning panes show what that costs.
Across many holdings the model spends substantial thinking on three questions the schema has already
settled:

- **Whether to wrap the answer in a markdown fence.**
  It argues both sides at length, reverses itself repeatedly, and settles on raw JSON —
  a decision the grammar makes for it, since a fence cannot be emitted under `format`.
- **What the key names are.**
  In its own words: *"It doesn't give me the keys.
  I have to derive them from the instructions."*
  It then invents candidate key sets (`model_sub_scores`, `conviction_level`, `horizon_reads`,
  `thesis_ledger`) and reasons about which an evaluator might expect.
- **How many sub-scores to emit.**
  It re-reads the four-sub-scores sentence three or more times, unsure whether momentum is included,
  before concluding correctly.

None of this reaches the output, because the grammar fixes all of it.
It is pure thinking-token expenditure, and thinking shares the same 131,072-token budget as the prompt
and the answer.
This finding therefore compounds Finding 1 rather than sitting beside it: the budget that ran out at
construction was partly spent deliberating a structure that was never in question.

The fix candidate is to state the contract in the prompt — the key list at minimum, ideally the schema —
and to replace *"the required JSON object"* with a sentence that names it.
The instruction to avoid markdown fences can be dropped entirely rather than clarified, since the grammar
already forbids them; saying nothing is better than saying something the model will litigate.

## Finding 3 — internal version vocabulary leaks into the prompt

`pipeline.rs:1531` renders the literal string
`"conviction/outlook/action not recorded (pre-v7 run)"` into the interpretation prompt.
The model quoted it back: *"This is the first Model Arm read (pre-v7 had no model arm recorded)."*

`v7` is this project's internal build vocabulary.
It carries no meaning the model can act on, and it invites more of the same over time — a prompt is a
contract with the model, not a changelog.
The fix candidate is to state the fact without the version: the prior run recorded no conviction,
outlook or action.

The neighbouring grade-band recalibration NOTE (`pipeline.rs:1907`) is **not** an instance of this and
should stay.
It conveys an analytical fact the model needs — a letter moved without an input moving — and the model
used it correctly.
The distinction worth holding is between telling the model about the portfolio and telling it about the
codebase.

## Finding 4 — the holding header omits units and often the company name

The header is built at `pipeline.rs:1429`:

```
"HOLDING: {} ({})\nQuantity: {}  Cost basis: {:.0}  Market value: {:.0}\n"
```

Two defects fall out of that one line.

**The name slot is frequently empty.**
It is filled from `d.position.description`, the Schwab-supplied description, which for PSX rendered as
`HOLDING: PSX ()`.
On ALAB the model had no name to work with and speculated in the pane that the ticker *"doesn't match
standard tickers for Alibaba (BABA)"* and might be *"mislabeled in the prompt"*, before correctly
deciding to trust the provided data.
The company name is already fetched — the FMP company-profile call is issued per holding — so the fix
candidate is to fall back to the profile name when the Schwab description is empty.

**Cost basis and market value carry no units and no per-share/total marker.**
They render as bare integers.
On PSX the model worked through whether `Cost basis: 524` was a per-share or a whole-position figure,
derived the per-share cost by division, cross-checked it against the retrospective's `price now 215.52`,
and only then proceeded.
The figures were correct throughout; the ambiguity was entirely in the rendering.

This is systematic rather than a one-off.
SBUX drew the same detour independently: *"cost basis 608 total / 5.6411 shares = $107.78 avg?
Wait, Market Value is 591, Qty 5.6411 -> Price ~104.77"*, followed by an explicit re-derivation of both
figures before any analysis began.
Two of the few panes reviewed show the model paying this cost, on different holdings.
The fix candidate is to label them as totals and mark the currency.

## Finding 5 — the engine arm shows a menu, not a choice

The interpretation prompt renders `ENGINE LEAN SET` (`pipeline.rs:1831`) as the set of rungs the engine
arm restricts itself to, explicitly framed as evidence rather than a bound.
It does not state which rung the engine arm actually took.
The model noticed: *"It does not explicitly state which one was picked in the text provided."*

The prior run's engine arm **is** rendered with its chosen action (`pipeline.rs:1534`), so the model sees
a pick for last time and only a menu for this time.
Whether the current engine arm's stand-in action exists at Step 6f, and whether showing it would anchor
the model arm in a way `portfolio-v7` deliberately avoids, is a design question this run surfaces rather
than settles.
It needs a ruling before it becomes work.

## Finding 6 — Portfolio request rows route to a report-pipeline step that does not exist

Every per-holding data request renders under a step labelled **Baseline market data**, and the tracker
header names that step even while a per-holding step is the one running.
Both symptoms have a single cause in the frontend.

`requestStep()` (`src/App.vue:196`) routes request rows by the event's `group`, and its groups are the
**report** pipeline's: `news`, `filter`, `routing`, `research`, `analyst`, `memory`.
Everything else falls through to `ensureStep(trace, "baseline", "Baseline market data")`.
The Portfolio job's groups — FMP, SEC, Stooq, FRED, local — match none of them, so every portfolio
request row lands in that fallback.
The step is **synthesized by the frontend**; the Portfolio backend never starts it and never finishes it,
so it stays open for the whole run and the header keeps naming it.

The routing is fixable without new backend events.
`dossier::assemble` runs at `job.rs:971`, inside the per-holding step opened at `job.rs:774` and closed at
`job.rs:1009`, so the owning step is already running when the rows arrive.
The `memory` branch of the same function already implements exactly the needed rule — follow the step that
is running — and the fix candidate is to make that the default, leaving the synthesized baseline step as a
last resort when nothing is running.
That places each holding's fetches under its own `Analyze <SYMBOL>` step and stops the phantom step from
being created at all.

## What the attempt did confirm

- **128 K runner stability — confirmed.**
  After 2 h 46 m of continuous load, `ollama ps` still reported a single `qwen3.5:122b-a10b` runner at
  context 131072, 100 % GPU, `UNTIL: Forever`.
  No reload, no CPU spill, no second runner: `keep_alive: -1` residency and the one-`num_ctx`-per-model
  rule both held across the whole run.
- **The run reached Step 7b at all**, so the gate, the live Schwab pull, the FRED rate-anchor hard-fail
  gate, the per-holding pass and Step 7a all completed on a live book.
  Their *quality* is unread, because none of it persisted.
- **The prior-run decode path is sound.**
  A v2-vintage run still decodes against current types, and the job's `.ok().flatten()` at
  `job.rs:566` means a decode failure would fail-soft to "no prior snapshot" rather than stop the run.

## What remains unread

Everything else in `verification/big-run-watch-set.md`.
The grade, valuation and target groups, the ledger and quick-check groups, selective re-analysis,
the pre-profit overlay, outcome learning, the two-arm read, listing and identity shapes, and every
data-source probe are all still first-read items.

## Residue

- **A pre-run gate failure, test-only.**
  `fred::tests::latest_rate_anchor_skips_markers_but_bounds_the_fallback` failed before the run.
  The test built its fixture dates from `Utc::now().date_naive()` while the bound under test dates on the
  ET session, so during the evening-ET window after the UTC rollover its "stale" fixture landed exactly on
  the floor and read fresh.
  The product code is correct and its comment predicts the case.
  The test now builds from `market_clock::et_session_date`.
  It is a latent flake that only fires between UTC rollover and local midnight.
- **A failed run is invisible to the obvious watchers.**
  It writes no `portfolio_runs` row and leaves the app process alive, so a watcher keyed on either
  misses it entirely.
  `job_runs` is the surface that carries the failure, and future run-watching should key on that.
- **The tracker captures are the only surviving record of the per-holding pass.**
  They are session-scratch, not committed.
  The quoted reasoning above is drawn from the 21:46:51 and 20:04:31 captures.
- **The Portfolio adapters emit no stdout diagnostics.**
  The captured app log holds 17 lines, all Tauri startup, and not one adapter line for a 2 h 46 m run.
  The `eprintln!` sites that exist belong to the report pipeline's adapters, so a Stooq throttle, an FMP
  429 ladder engagement, or a degraded fetch leaves no trace outside the run row that a failed run never
  writes.
  This is why the failure analysis rests on screenshots.
- **The Ollama server log is the most durable evidence a failed run leaves.**
  It survived independently of the app, dates every call, and is what made the construction-cost and
  throughput numbers above recoverable.
  Future runs should keep it deliberately rather than incidentally.
