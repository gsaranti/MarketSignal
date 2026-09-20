# Big confirmation run — findings (2026-09-20, attempt 8, ended at 3 of 47)

This record holds only what attempt 8 showed needs to change, plus the two latency decisions the user took on it.
Every finding names the defect, the evidence, the cause as far as it is established, the change proposed, the check that admits it and the stamp it moves.
What read clean is not repeated here; the per-holding reads and the running notes are in the archive's `attempt-8-watches.md`.
Each finding is proposed and unruled unless a `Ruled` line says otherwise.

Run identity: progress id `0d2bf4ec-3d95-4d78-a531-fea102dd2c13`, portfolio run `0537f566-f6f3-4afd-892f-32637ab88ffe`, `job_runs` id 8.
Launched 2026-09-19 22:16 PDT (05:16:58Z) from the store re-wiped to a clean debut the day before, on the 47-position book (33 stocks, 14 funds).
Debut stamps confirmed from the persisted checkpoint header: `portfolio-v48` / `checkpoint-v15` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`, `prior_run_id` null.
User-ended by a cooperative run-tracker cancel at 07:23:54Z (126 m 56 s) after TSLA, PSX and SPMO completed, ARKF cut at its fifth gathering turn.
The run was ended for the hour, not on a finding: neither ruled stop rule fired, no unit exclusion appeared on the three holdings, and the first-holding oscillation read was clean.
Attempt 7's Findings 1, 2 and 4 admitted on their checks: the serve log restored a context checkpoint 171 times at 164 distinct positions (attempt 7: 26 at 18); every stock synthesized all six root topics (attempt 7: two); all three action rationales name their tested returns by value.
Attempt 7's Finding 3a did not admit; it returns below as Finding 2.
Archive: `~/Downloads/market-signal-attempt-8-logs/` (dev log, Ollama serve log, the four fenced thought logs, read-only store extracts, `attempt-8-watches.md`).
The comparison set is attempt 7's TSLA and PSX (`~/Downloads/market-signal-attempt-7-logs/`) under `portfolio-v47`.

## Findings that need a change

### Finding 1 — Synthesis responses that open with the claims array fail the parse, and the record does not say why

- Evidence.
  Three of the run's 26 synthesis calls failed `parse_findings_wire` and were retried once, each retry succeeding: TSLA competitive-position (call 9, 05:19Z), SPMO fund-mandate-manager (call 157, 07:00Z) and SPMO fund-exposure-profile (call 183, 07:13Z).
  All three bodies share one shape: the object opens `{"claims": [` with no `findings` key in the logged head, the body is short enough to hold claims alone (2,498, 1,303 and 1,303 characters) and the call stopped on its own (`done_reason` stop).
  Every successful synthesis follows the shape template's order, `findings` first.
- Cause, as far as the record allows.
  `findings` is in the grammar's `required` list (`research.rs` `findings_schema`) and the call carries `format`, so the key cannot be omitted; the parser's own guard rejects a blank `findings` ("carried a blank `findings` field").
  The likeliest reading is that a claims-first emission ends with an empty findings string.
  The record cannot confirm it: the retry line formats the error with `{err}` (`local_model.rs`, `retrying once`), which prints only the outermost context, and `body_snippet` (`research.rs`) keeps a 400-character head, so neither the serde message nor the guard's message nor the body tail survives.
  The persisted `model_retries` entry carries the class string alone ("content failed its parse"), so the data-health read cannot attribute it either.
- Change proposed.
  Telemetry: the retry line and the persisted retry cause carry the full error chain (`{err:#}`), and the snippet keeps a head and a tail of the body.
  Prompt: the synthesis task states that `findings` is written first and is never empty, before the claims.
- Check, on attempt 9.
  Any synthesis parse failure names its cause in the dev log and in `model_retries`; no failed body opens with `claims`.
- Stamp: `portfolio-v49` for the task sentence; none for the telemetry.

### Finding 2 — Fact-period values violate the stated formats, and the gap records neither the kind nor the value

- Evidence.
  Attempt 7's Finding 3a check read "zero `invalid fact period` gaps on attempt 8's first stocks"; TSLA persisted nine and PSX eight, each demoting a claim's period to `unknown`.
  The one rejected value the log shows is `{"kind":"quarter","value":"Q1 2026"}` (TSLA call 9) against the prompt's `calendar YYYY-Qn, e.g. 2026-Q2`.
  The thought logs quote the format correctly in three TSLA syntheses and write `2026-Q2` correctly in a fourth, so the model knows the rule and drifts from it at the point of writing.
  The other 16 rejected values are unattributable: the gap line reads `invalid fact period retained as unknown` with no kind or value.
- Cause.
  The rejected value copies the source's own wording.
  Two prompt features invite that: the neighbouring `fiscal` kind asks for "the source's exact fiscal-period label, e.g. Q4 FY2025", and the JSON shape template gives `"value":""` with no format hint (`research.rs`, the shape string), so the format lives only in prose well above the point of writing.
  The "use quarter only for a stated calendar quarter" clause governs kind choice, not value format; it cost deliberation on TSLA call 19 (a `6/30/2026` table column, a `Q2 FY26` label) and the model honoured it.
- Change proposed.
  The shape template carries the value format inline: `"value":"<YYYY-MM-DD|YYYY-MM|YYYY-Qn|YYYY|source label|empty>"`.
  The fiscal gloss no longer says "exact", so it does not prime copying source wording into the other kinds.
  The gap line records the rejected kind and value.
- Check, on attempt 9.
  Zero `invalid fact period` gaps on the first stocks; any that appear name the pair that was rejected.
- Stamp: `portfolio-v49` (shape template and gloss text); none for the gap telemetry.

### Finding 3 — The normal distillation ceiling binds on a third of holdings, and the wasted first pass costs more than the guardrail protects

- Evidence.
  TSLA's distillation stopped at exactly 8,192 tokens (`done_reason` length) after 5 m 33 s and took the one expanded re-attempt at 32,768, which finished at 8,055 tokens in 4 m 20 s; PSX (5,733 tokens) and SPMO finished in one pass.
  The expanded pass restored the 31,306-token prompt from cache in 69 ms, so the whole first pass was the waste.
- Cause.
  `NUM_PREDICT_DISTILL` is 8,192 as the runaway and latency guardrail (`pipeline.rs`); the deadline derives from that reservation, the expanded ceiling only fits on the 128 K reasoner, and the exact-reservation stop is the data-health signal for an oversized distillation.
  With a six-topic research shape the normal output sits near the ceiling, so the guardrail fires on ordinary work.
  The live roster has no 32 K fast tier, so the fit argument does not bind today.
- Change proposed.
  Ruled 2026-09-20: raise the normal ceiling to a value in the 12,288–16,384 band so a six-topic distillation fits in one pass, keeping the exact-reservation expanded re-attempt as is.
  The exact value is a plan flag.
- Check, on attempt 9.
  No expanded re-attempt on a holding whose first pass produced under the new ceiling; the data-health read still counts any that fire.
- Stamp: none (a constant; no prompt or persisted-shape change).

### Finding 4 — Each fresh conversation re-prefills its whole prompt: topic root turns, syntheses and the three closing calls carry no restorable prefix

- Evidence.
  Prompt evaluation cost 6.3 min on TSLA and 6.1 min on PSX; 4.5 and 5.0 of those minutes are the calls that evaluated a large prompt at under 3,000 tokens per second: each topic's first gathering turn (5 per stock), each synthesis (3–5) and distillation, interpretation and action (1 each).
  Append-only turns inside a pass restored from the previous checkpoint and evaluated only the new tool result, as attempt 7's Finding 1 required.
- Cause.
  By design each topic pass and each synthesis is a fresh conversation (`docs/portfolio-workflow.md`, topics do not share a conversation), so the only shared prefix is the holding header; the topic questions follow it and the evidence block follows those, so nothing after the header is restorable across passes.
- Change proposed.
  Ruled 2026-09-20: order each fresh conversation so the holding-constant text precedes the topic-variable text, and give the synthesis the same evidence order as its pass, so the header and the evidence already prefilled form the restorable prefix.
  The task text does not change in meaning; only its order moves.
  Ruled 2026-09-20, not taken for now: overlapping web fetches with model time, thinking off on gathering turns, a synthesis thinking cap and a lower gathering turn cap.
- Check, on attempt 9.
  Synthesis and topic-root prompt evaluation restores a checkpoint past the header; per-stock prompt-evaluation time drops below the attempt-8 band.
- Stamp: `portfolio-v49` (message order moves).

### Finding 5 — Two action-packet lines the model reads as ambiguous on every stock

- 5a. The quality sub-score scale.
  PSX's action trace: "Quality 8 vs Valuation 96 seems inconsistent if 0-100 … maybe a typo in input"; it then decides to trust the text.
  Computed quality was 7.7 on PSX and 10.0 on TSLA, with the model's own view at 6 and 28, so the doubt recurs wherever the score is low.
  Change proposed: the sub-score line states the scale and polarity once; separately, the quality score's construction is read across a fuller book before any calibration change (`grade-v2.3`).
- 5b. The supported-actions clause.
  "The rungs the computed read supports on its own" drew three markers on each stock; the trace tries to derive the set from capital efficiency instead of reading the list it is given.
  Change proposed: the clause names the list as given and says it is not derived.
- Check, on attempt 9.
  No action trace questions the score scale or derives the supported set.
- Stamp: `portfolio-v49` (shared).

## Slices

The five findings are handled as two tasks, prompt content first and prompt order second, so the two never edit the synthesis prompt in opposite directions.
Ruled 2026-09-20: the slices share one combined delivery stamp, `portfolio-v49`, while remaining separately planned and implemented tasks.
Neither slice is planned yet; each takes its selector rulings at its own plan.

- Slice A — the synthesis contract and the action-packet glosses (Findings 1, 2, 5).
  The findings-first task sentence, the value-format hint in the shape template and the softened fiscal gloss (Findings 1, 2); the sub-score scale line and the supported-actions clause (Finding 5); and the two telemetry changes, the retry line and persisted cause carrying the full error chain with a head-and-tail body snippet, and the fact-period gap naming the rejected kind and value.
  Findings 1, 2 and 5 are proposed and unruled, so this slice carries the selector rulings.
  It lands first because it changes what the synthesis prompt says.
  Stamp: `portfolio-v49`; none for the telemetry.
- Slice B — latency (Findings 3, 4).
  The normal distillation ceiling moves into the 12,288–16,384 band, the exact value a plan flag (Finding 3); each fresh conversation orders its holding-constant text before its topic-variable text and the synthesis carries its pass's evidence order (Finding 4).
  Both are ruled; the remaining latency levers are ruled not-taken and stay out of the slice.
  It lands second because it changes where the synthesis prompt's parts sit, and works on Slice A's final text.
  Stamp: the shared `portfolio-v49` for Finding 4's message order; Finding 3 moves nothing.

## Observed, no change proposed

- Follow-ups and the disconfirming pass were "not spent: budget exhausted" on five of six TSLA topics and on PSX; roots ran first as Slice 1 guarantees, so the 40-fetch budget is now the binding constraint.
  The calibration read waits for a fuller run.
- Distillation dropped claims for unshown or unknown evidence references (12 on TSLA, 3 on PSX); a citation-resolution watch.
- 17 of 35 TSLA fetches failed on paywalled or bot-refusing hosts (Barron's 401, Tesla's robotaxi page 403, Reuters, Fool, Seeking Alpha); none was an issuer earnings page, so Finding 5's EDGAR route had nothing to recover.
- SPMO, a fund, took about 40 minutes like PSX: 62 gathering turns and ten syntheses, with interpretation at 2,421 words and 10 markers where the stocks read 0.
- Per-stock wall time was 57 min (TSLA) and about 40 min (PSX); attempt 7's 37-minute TSLA covered two topics.
- The re-think markers that remain are source-conflict deliberation in synthesis (two sources disagreeing on Tesla's Q2 EPS estimate) and capital-efficiency weighing in action; interpretation carried none on either stock.
- All 33 stock rows again arrived with an empty issuer description (the standing ingestion watch).
- CFTC positioning returned: eight contracts, no gaps.

## Run configuration and boundaries

- Ollama v0.32.5, one slot, flash attention, fresh daemon and serve log (`ollama.log`); runner `n_ctx` 131072; the 122B at 100% GPU.
- SearXNG pinned image with the Serper key rendered; bring-up probe: serper 10, google 10, bing 10, reuters 9; qwant and duckduckgo CAPTCHA; google cse suspended; mojeek 0.
- Store after the cancel: TSLA, PSX and SPMO checkpoint rows, `portfolio_runs` 0, `job_runs` max id 8, `web_source_state` 39 host rows, `web_documents` 47; the prod store untouched.
  Attempt 9 re-wipes first.
- Whole-run boundaries: `job_runs` 8 `started_at` 2026-09-20T05:16:58Z, `finished_at` 2026-09-20T07:23:54Z; research `elapsed_secs` 1,962 (TSLA), 1,914 (PSX), 1,881 (SPMO) with fetches spent 35, 28 and 19.
