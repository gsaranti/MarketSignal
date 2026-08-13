# Big confirmation run — attempt 2 — analysis plan

This is the forward-looking plan for the analysis of big-run attempt 2, to be executed next session.
Like the watch set, it is written before the work it directs; the analysis then produces the dated record `2026-08-13-big-run-attempt-2.md` beside it.
The run itself completed the full 47-holding pass and then failed at Step 7b construction; this plan says how to turn that outcome into a fix that produces a book, plus two secondary reads (prompt effectiveness, result accuracy).

## The run in one screen (self-contained context)

Everything below is on disk in the **dev** store; the analysis is pure read-only and needs no app run, no model serving, and no computer use.

- **Run identity.** Progress/execution run id `3f42e8e5` (used to stamp the thought-log dir); persisted `portfolio_runs.run_id` = `6a52f1dd`, row `id = 2`, `created_at` 2026-08-13T19:16:02Z.
  The two ids differ — confirm whether that split is by design (the thought-log sink keys off the `RunContext` id, the persisted row off a freshly minted portfolio run_id) or a latent identity seam worth a note.
- **Terminal state.** `finished: failed — portfolio construction jointly infeasible after the named-violation re-run`.
  The Step 7b repair engaged: the pass persisted as a **degraded run** (`constructed = 0`, mirrored in both the JSON `constructed: false` and the store column), excluded from `latest_run`, with `outcome: null`.
  This is the decisive improvement over attempt 1, which persisted nothing.
- **The failure, precisely.** Construction's divergence-cause validation rejected five names:
  - AMZN — final action `trim` departs standalone lean `hold`, **no `divergence_cause`**.
  - DIS — final action `hold` departs standalone lean `add`, **no `divergence_cause`**.
  - GM — final action `trim` departs standalone lean `hold`, **no `divergence_cause`**.
  - RKT — context cause `cash-freed` **maps to no real aggregate** (must be checkable against the whole-book aggregates).
  - TDOC — context cause `cash-freed` **maps to no real aggregate**.
  The named-violation re-run repaired only those five and they failed again → jointly infeasible → fail-hard.
- **What did NOT recur from attempt 1.** No output-budget exhaustion, no truncation: construction sub-call prompts were ~9.5K–12K tokens against `num_ctx` 131072, `truncated = 0`; the per-holding pass ran 47/47 with zero 429s, retries, or fetch errors, and every `company-eod-deep` (the Stooq-replacement rung) succeeded.
- **Distributions banked** (46 priced + 1 role-risk-only): grades B:9 C:13 D:16 F:8, **no A**; risk-tier High 28 / Medium 12 / Low 6; dead-money fails 14 / clears 9 / indeterminate 23.
- **Open question resolved.** SBUX is not degenerate: coherent steep-bearish, 12-mo base ~$65 vs spot ~$105, wide band $52–$102, full `targets-v3` methodology, grade F, dead-money `fails`.

### Evidence locations and re-hydration

Do this first each analysis sitting; the app holds the DB open in WAL, so copy it out rather than reading it live.

- **Dev store dir:** `~/Library/Application Support/com.georgesarantinos.market-signal/dev/`
- **Copy-out (safe read):**
  ```sh
  DEV="$HOME/Library/Application Support/com.georgesarantinos.market-signal/dev"
  TMP="$SCRATCH/dbcopy"; mkdir -p "$TMP"; rm -f "$TMP"/market_signal.db*
  cp "$DEV"/market_signal.db "$TMP"/ 2>/dev/null
  cp "$DEV"/market_signal.db-wal "$TMP"/ 2>/dev/null
  cp "$DEV"/market_signal.db-shm "$TMP"/ 2>/dev/null
  DB="$TMP/market_signal.db"
  ```
- **Thought-logs:** `$DEV/thought-logs/20260813-191600-3f42e8e5/` — `construction.txt` (46 KB) plus 38 `holding-<SYM>.txt` streams.
  These are the user-owned diagnostic; read them here as evidence for the analysis.
- **Useful extraction queries** (all read-only against the copy):
  ```sh
  # all 47 verdict symbols
  sqlite3 "$DB" "select group_concat(json_extract(value,'\$.symbol'),' ') from portfolio_runs, json_each(portfolio_runs.run_json,'\$.verdicts');"
  # one holding's full disposition (two arms)
  sqlite3 "$DB" "select json_extract(value,'\$.disposition') from portfolio_runs, json_each(portfolio_runs.run_json,'\$.verdicts') where json_extract(value,'\$.symbol')='AMZN';"
  # one holding's audit (engine metrics ground truth)
  sqlite3 "$DB" "select json_extract(value,'\$') from portfolio_runs, json_each(portfolio_runs.run_json,'\$.audit') where json_extract(value,'\$.symbol')='AMZN';"
  # roll-up + whole-book aggregates (cash-freed checkability)
  sqlite3 "$DB" "select json_extract(run_json,'\$.roll_up.aggregates') from portfolio_runs;"
  sqlite3 "$DB" "select json_extract(run_json,'\$.roll_up.data_health') from portfolio_runs;"
  ```

## Workstream 1 — How to get the book produced (PRIMARY)

The whole point of the next session.
Construction fail-hards on divergence-cause validation and the named-violation re-run cannot repair it, so no book is ever emitted.
Root-cause the two failure classes, then decide the fix that lets a legitimate divergence pass while a fabricated one still fails.

### The two failure classes

1. **Missing cause** (AMZN, DIS, GM): the model changed the action away from the lean but supplied no `divergence_cause` at all.
2. **Uncheckable cause** (RKT, TDOC): the model supplied `cash-freed`, but the validator found no whole-book aggregate that corroborates it.

### Hypotheses to test (each with its read)

- **H1 — Prompt salience.** The construction prompt under-states that any action ≠ lean *requires* a cause, or under-explains each cause's meaning and how it is checked.
  Read `construction.txt` for whether the model even attempted a cause for AMZN/DIS/GM, and read the construction prompt/system text in `src-tauri/src/portfolio/construction.rs` (and any prompt module it pulls) for how the requirement is worded.
- **H2 — Schema does not force it.** `divergence_cause` is optional in the construction output schema, so the model can omit it under load.
  Check whether the schema/grammar can make the cause structurally required whenever `action != lean` (a conditional-required field, or a post-parse rejection that feeds the re-run a precise instruction).
  If yes, the missing-cause class becomes deterministically impossible.
- **H3 — `cash-freed` is unsatisfiable under the fixed preset.** BUILD records that cash-residual drawdown is inert while the fixed preset leaves cash unconstrained.
  If the whole-book aggregates never move cash, then `cash-freed` can *never* be checkable and the vocabulary offers the model a cause it is structurally forbidden to use.
  Inspect `roll_up.aggregates` and the checkability logic to confirm or refute; if confirmed, this is a real contract bug, not a model error.
- **H4 — Cause vocabulary too narrow.** The three causes are {became-oversized, overlap-emerged, cash-freed}.
  GM (grade F, dead-money `fails`) and RKT/TDOC may have a legitimate reason to diverge that none of the three expresses (e.g., a conviction/quality/risk-tier downgrade or a dead-money exit), forcing the model to either omit or misuse a cause.
  Read the five names' `construction.txt` reasoning for the *intent* behind each divergence and judge whether the vocabulary can express it.
- **H5 — Re-run feedback is too thin.** The named-violation re-run repaired only the five names but they failed identically, which suggests the re-run may not hand the model the specific violation to correct.
  Trace the re-run path (the "named-violation re-run" in `construction.rs`) for what context it feeds back.
- **H6 — Fail-hard is too strict for this stage.** Even with H1–H5 fixed, a model that cannot justify a divergence should perhaps fall back to the standalone lean action for that one name (accept the lean, drop the divergence) rather than fail the whole book.
  This is a design ruling for the user: is a book with a few names forced-to-lean strictly better than no book?
  Frame the trade-off precisely (it touches the model-arm-never-binds-engine boundary only on the action, not the intrinsic verdict).

### Deliverable for workstream 1

A ranked set of fix options with a recommendation, each labelled code-fix vs ruling-needed:
- the deterministic schema-forcing of `divergence_cause` (H2),
- the `cash-freed` checkability fix or its removal from the vocabulary under the fixed preset (H3),
- any vocabulary extension (H4),
- the re-run feedback improvement (H5),
- and the per-name fallback-to-lean relaxation (H6),
with a clear statement of which combination is expected to actually produce a book, and which items need a user ruling before implementation.
If a code slice is obvious and low-risk (e.g., H2), draft it; otherwise stop at the proposal and bring the rulings to the user.

## Workstream 2 — Prompt effectiveness (thought-logs, 5–10 files)

Read a small, deliberately diverse subset and judge how well the prompts steer the model.

### Which files

- **`construction.txt`** — mandatory; it is also the primary evidence for workstream 1.
- The failing names with logs: **AMZN, DIS, RKT, TDOC** (GM has no stream — note it).
- A grade spread: one clean **B** (e.g., AMZN), a **C** (DIS), a **D** (RKT/TDOC), an **F** (SBUX or GM-adjacent; SBUX has a log).
- One **fund** (e.g., QQQ or SCHA) and, if present, the one **role-risk-only** name, to see the discriminated-union branch reason.
- One **short** stream (LCID, NVDA, PBW, or ARKQ are ~4 KB) to learn why the reasoning is so brief — a light path, a role-risk skip, or truncated thinking.

Keep the total at 5–10 for context budget; construction plus ~6–8 holdings.

### What to assess

- Does the reasoning show the model actually applying the interpretation-prompt contract — the two-arm separation, the grade/target methodology, the dead-money/hurdle read, the lens library as discipline?
- Where does the prompt appear to under-specify, mislead, or invite the model to fabricate (especially the divergence-cause and `cash-freed` semantics)?
- Are there signs of the model straining against the schema, misreading a field, or padding to satisfy a required section?

### Deliverable for workstream 2

A short list of concrete prompt/contract weaknesses with the exact stream excerpt that evidences each, feeding the workstream-1 fix and any interpretation-prompt tightening.

## Workstream 3 — Result accuracy spot-check (30–50% of holdings)

Sample ~18–24 of the 47 and judge whether the verdicts are believable.
These are predictions, so there is no ground truth; the value is the frontier-model catch of anything that is internally contradictory or externally implausible.

### Definition of "accurate" (three tiers)

1. **Internal coherence** — the verdict must not contradict itself:
   - grade vs sub-scores (an F with all-high sub-scores is suspect);
   - action vs lean vs dead_money vs conviction;
   - `horizon_outlook` direction vs `price_targets` direction;
   - target sanity — base within [bear, bull], 12-mo base sensibly related to spot and to the stated `methodology` string, no flat/degenerate or inverted bands;
   - the two arms (`model_view` vs `engine_view`) diverging sanely, not wildly.
2. **Model-vs-engine cross-check** (the rigorous, data-grounded test) — compare the model's stated fundamentals in `financial_summary` / `model_view` against the deterministic engine `metrics` in `audit[]` for the same symbol.
   A model P/E, margin, or growth claim that disagrees with the engine's computed metric is a hallucination or misread — flag it.
3. **External plausibility** — does the fundamental read match what is broadly known about the company?
   Catch a grade or target that is defensible-nonsense: an F on a structurally healthy megacap, a B on a cash-burning pre-revenue name, a target implying an implausible move with no basis, a P/E that is wrong for the name.

### Sampling frame

Cover the space, do not just take the first N:
- every grade bucket (≥2 each of B/C/D/F, plus the role-risk-only name);
- both extremes of risk-tier and of dead-money (fails / clears / indeterminate);
- priced equities **and** at least three funds (fund path is under-exercised);
- the five construction-failing names (already required by workstream 1);
- the steep/extreme targets specifically (largest implied up/down moves).

### Deliverable for workstream 3

A ranked list of any verdicts that look "way off," each tagged by tier (internal / model-vs-engine / external), with the specific numbers and a one-line why.
A clean bill for the rest is itself a result — record the count checked and the count flagged.

## Synthesis and deliverables

1. **The dated record** `docs/verification/2026-08-13-big-run-attempt-2.md` — the run outcome, the three workstream findings, and a **watch-set reconciliation**: which `big-run-watch-set.md` items are now confirmed (the per-holding half, deep-EOD/429, num_ctx, fund path, degraded persistence, grade/risk/dead-money distributions, SBUX non-degeneracy) and which remain unexercised because construction failed (the book, outcome learning, the two-arm retrospective, the paired-card render, all between-runs surfaces).
2. **The book-production fix proposal** from workstream 1, with rulings surfaced to the user before any implementation.
3. **Prompt/accuracy notes** folded into the record and into any interpretation-prompt slice.
4. **Memory + handoff updates** (user-run): fold the attempt-2 outcome into the big-run saga memory and set the next step to the fix slice.

## Constraints and guardrails

- **Read-only, dev store only.** The analysis needs no run; do not launch the app, do not serve models, and never touch prod (the request_access / open_application path resolves "Market Signal" to the prod bundle — see the dev/prod identity-collision memory).
- **Do not re-run the job** to "get a book" until a fix lands; the persisted degraded run and the thought-logs are the whole evidence base and are durable.
- **If code changes,** the verification bar is the full set for touched Rust: `cd src-tauri && cargo test` and `cargo clippy --all-targets --all-features`, plus `npm run build` / `npm test` if any frontend module is touched.
- **Single-home any new contract** and keep docs prose sentence-per-line, per CLAUDE.md.
