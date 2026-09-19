# Big confirmation run — findings (2026-09-19, attempt 7, ended at 2 of 47)

This record holds only what attempt 7 showed needs to change.
Every finding names the defect, the evidence, the cause as far as it is established, the change proposed, the check that admits it and the stamp it moves.
What read clean is not repeated here; the per-holding reads, the stop-rule counts and the run timeline are in the archive's `notes.md`.
Ruled 2026-09-19: attempt findings live in a dated record like this one, focused on issues and required changes; the 2026-09-17 work list stays the record of the fixes that preceded this attempt and no longer takes new entries.
Each finding below is proposed and unruled; the user rules each through the selector before a plan starts, as the standing rule requires.

Run identity: progress id `1e35a182-4943-46e8-aceb-7f9e5355465f`, portfolio run `136d171e-edf4-4ef9-85c6-974f4f58999c`, `job_runs` id 7.
Launched 2026-09-19 14:46 PDT (21:46:41Z) from the store re-wiped to a clean debut that morning, on the 47-position book (33 stocks, 14 funds).
Debut stamps confirmed from the persisted checkpoint header: `portfolio-v47` / `checkpoint-v14` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`, `prior_run_id` null.
User-ended by a cooperative run-tracker cancel at 23:05:31Z (78 m 50 s) after TSLA and PSX completed, SPMO cut at its start.
Neither ruled stop rule fired: the unit-exclusion rate was unread on two priced stocks, and the first-holding oscillation read was clean.
The run was ended on Finding 2 — with two of seven topics researched per stock, the remaining 45 holdings would have measured a known coverage loss rather than confirmed the job.
Archive: `~/Downloads/market-signal-attempt-7-logs/` (dev log, Ollama serve log, the three thought logs, read-only store extracts, `notes.md`, `README.md`).
The comparison set is attempt 6's TSLA and PSX (`~/Downloads/market-signal-attempt-6-logs/`), the same holdings under `portfolio-v35` with one pass per topic.

## Findings that need a change

### Finding 1 — Every gathering turn re-prefills the whole conversation: the per-turn rewrite of the first user message defeats the runtime's context cache

The local model is not slower; the work handed to it per turn is five to thirteen times larger than in attempt 6, and gathering turns are where attempt 7's extra time went.

| Stage, per holding | Attempt 6 TSLA | Attempt 7 TSLA | Attempt 6 PSX | Attempt 7 PSX |
| --- | --- | --- | --- | --- |
| Gathering turn 1 (n, median, sum) | 7 · 10 s · 79 s | 6 · 26 s · 161 s | 7 · 9 s · 70 s | 5 · 30 s · 135 s |
| Gathering turns 2+ (n, median, p90, sum) | 46 · 2 s · 8 s · 221 s | 33 · 26 s · 34 s · 899 s | 48 · 5 s · 11 s · 297 s | 33 · 31 s · 43 s · 1,069 s |
| Synthesis (n, median, sum) | 7 · 66 s · 495 s | 6 · 108 s · 697 s | 8 · 97 s · 739 s | 5 · 77 s · 450 s |
| Distillation | 106 s | 197 s | 96 s | 168 s |
| Interpretation | 76 s | 55 s | 227 s | 213 s |
| Action | 94 s | 107 s | 122 s | 75 s |
| Model time, all calls | 1,071 s | 2,116 s | 1,551 s | 2,110 s |

Durations are the dev log's `ok after` figures per call; attempt 7 spent its model time on two topics, attempt 6 on seven.

- Evidence, runtime side.
  The Ollama serve log prints the tokens each request actually prefilled.
  Attempt 6: 319 requests, 226 of them under 1,500 tokens — the turn-2+ requests reused the cached conversation and evaluated only the new tool result (typical sequence 2502, 727, 794, 144, 150, 743, 118 …).
  Attempt 7: 98 requests, 90 of them evaluated the whole conversation, growing every turn (2744, 4032, 5746, 7621, 6079, 6844, 8608, 9534, 11126, 11887, 12632, 13257 …); gathering prompts ran 43,000–50,000 characters (11,900–13,400 tokens median) at about 560 tokens per second of prefill, so 20–30 s of every 26–31 s turn is prefill.
  Attempt 6 restored a context checkpoint on 280 of 319 requests at 266 distinct positions (the end of the previous turn); attempt 7 restored one on 26 of 98 requests at 18 positions, all of them a pass's constant prefix (2231, 2155, 240), never the previous turn.
- Evidence, code side.
  `run_pass` in `src-tauri/src/portfolio/research.rs` rebuilds `messages[1]` — the whole Part 1 message — before every gathering call with a decremented countdown (`SEARCHING / Replies remaining, including this one: N`), landed by entry 5 (`e00751e`) under the ruling "show replies left in this pass, refreshed before each gathering request".
  Every turn's prompt therefore differs from the cached conversation inside the first user message, before the tool turns the cache holds.
- Mechanism.
  Qwen3.5 is a hybrid model whose recurrent layers cannot be rewound to an arbitrary token; the runtime restores only from saved context checkpoints, which it creates at the end of each request.
  An append-only conversation continues from the last checkpoint at a cost of the new tokens; an edit inside an earlier message leaves no checkpoint before the divergence except the shared prefix from an earlier pass, so the runtime re-evaluates the entire conversation.
  This binds every long-context, multi-turn call on the local model: any per-turn mutation of earlier messages, however small, costs a full prefill on this model family.
- Cost.
  About 700–800 s per holding on gathering alone at two topics; at seven topics it would have been roughly 2,500 s per holding, or about 33 hours over the book before any other stage.
- Change proposed.
  The gathering conversation becomes append-only: the countdown moves out of Part 1 and rides the newest tool-result (or user) message each turn, or is dropped in favour of the existing eight-turn statement in the task.
  The reuse block and Part 1 render once per pass and are never rewritten.
  A regression test asserts that the messages from turn 1 are byte-identical prefixes of every later turn's messages.
- Check, on attempt 8.
  `prompt_eval_count` on gathering turns 2+ is bounded by the new tool result, not the conversation; the serve log restores a checkpoint on nearly every request at a distinct position; gathering turn-2+ median returns to the attempt-6 band (2–5 s).
- Stamp: `portfolio-v48` (the gathering message text moves a line).

### Finding 2 — Follow-up passes spend the holding's budget before later topics' root passes run: two of seven topics researched on both stocks

- Evidence.
  TSLA and PSX each ran competitive-position as three passes (root plus two follow-ups, each with its own synthesis: 2 m 47 s / 44 s / 2 m 09 s on TSLA) and results-revisions as three (TSLA) or one (PSX, its follow-up "not spent: budget exhausted").
  The 30-minute `MAX_WALL_PER_HOLDING` then bound (`elapsed_secs` 1,870 and 1,891) and both rows persist `topic catalysts-risks skipped: budget exhausted`, the same for management-capital-allocation, narrative-sentiment and forward-thematic, plus `disconfirming-fetch pass not spent: budget exhausted`.
  Fetches were not the binding budget (18 and 20 of 40).
  Attempt 6's TSLA ran seven topics in 60 calls and PSX eight syntheses because no follow-up fired; since `portfolio-v43` the synthesis shape carries `followup_question` as a placeholder and the model filled it on every root pass.
- Cause.
  The orchestrator honours each follow-up proposal in topic order as it arrives, so a topic's follow-ups run before the next topic's root pass; `docs/web-research.md` caps depth per topic (≤3 passes) but nothing orders root passes ahead of follow-ups across topics.
- Change proposed.
  Every eligible topic's root pass runs first, in priority order; follow-up proposals are queued and spent from the remaining budget afterwards, topic order preserved, the disconfirming pass keeping its place.
  Finding 1's fix may make three passes per topic fit the wall, but ordering is the guarantee, so both land.
- Check, on attempt 8.
  Seven topics synthesized on every stock that keeps its wall budget; the skip gaps, when they appear, name follow-ups before they name root passes.
- Stamp: none (orchestration order; no prompt or persisted shape changes).

### Finding 3 — Second-guessing: the questions the model re-litigates, and the prompt changes that close them

Re-think markers (wait / actually / re-read) per call class on TSLA and PSX, attempt 7 against attempt 6, remain a diagnostic and never the gate.
The attempt-5 tell (resolving "JSON or Markdown?" before planning) is gone on every call read, and the ENGINE SET clause that drove attempt 6's action re-think produced zero "engine" mentions in either action trace.
What remains is concentrated on four prompt questions, each with a concrete change.

| Class | Attempt 7 (TSLA + PSX) | Attempt 6 reference |
| --- | --- | --- |
| Gathering | 12 markers over 81 calls | PSX turns near zero |
| Synthesis | 35 over 11 calls (median 3, max 12) | PSX 40 over 8 calls (max 12) |
| Interpretation | TSLA 2, PSX 21 | TSLA 0, PSX 20 |
| Action | TSLA 9, PSX 3 | TSLA 17, PSX 17 |

- 3a. `fact_period` has no way to say "a quarter" (synthesis; ≥13 of the 35 markers; six persisted gaps).
  The synthesis shape offers kind `day`, `month`, `year`, `range` (both ends as YYYY-MM-DD), `fiscal` (the source's exact label) or `unknown`, and tells the model not to infer a calendar period from a fiscal label.
  A fact stated as "three months ending June 2026" or "Q4 FY2025" fits none: the model cycles year → fiscal → range → unknown (TSLA call 5 spent eight markers on one delivery figure; PSX call 64 four) and six model periods were rejected by the parser and persisted as `invalid fact period retained as unknown` (two TSLA, four PSX).
  Change: add kind `quarter` with value `YYYY-Qn` (or widen `fiscal` to name quarter labels explicitly) and gloss each kind with one example; the reconciliation rule in `docs/portfolio-analysis.md` gains the quarter as a comparable period.
  Check: zero `invalid fact period` gaps on attempt 8's first stocks and the quarter figures persist as quarters.
- 3b. `followup_technology_event` asks for a judgement the shape cannot define (synthesis; ≥6 markers on TSLA alone).
  The boolean is defined in the task as "true only when the follow-up concerns a competitor or supplier product or standard announcement"; the model re-argues the NHTSA rule proposal against that sentence on both TSLA competitive-position syntheses.
  Its only reader arms the conditional technology topic once per holding (`research.rs` `technology_event`), a trigger the engine's deterministic event pre-flag also serves.
  Change: drop the field from the synthesis shape and let the conditional topic arm from the pre-flag and the other defined triggers (`docs/portfolio-analysis.md` §Research agenda); if a model-side trigger is still wanted, it becomes a closed enum of the announcement kinds rather than a definition sentence.
  Check: the field is absent from the grammar and the wire; the conditional-topic arming tests pass on the remaining triggers.
- 3c. Interpretation re-litigates its relationship to the computed figures (PSX 21 markers, ten of them on price levels).
  The PSX trace debates whether "your own" targets should anchor on or diverge from the computed bands, re-derives the one-month proration from the twelve-month base, tries to reconcile "Grade D" with a valuation score of 96, and re-checks the "level the metric has not already crossed" rule against the spot.
  Attempt 6's PSX carried 20 markers on the same call, TSLA 0 both times, so this is a holding-shape effect the v40 rewrite did not reach.
  Change: remove the derived letter grade from COMPUTED SCORES (the model returns its own letter and the grade is app-computed downstream); state once that the computed bands are inputs and the rationale names the figure that differs; render the one-month computed band without its proration method sentence.
  Check: PSX-class interpretation markers read again beside 20/21 on attempt 8.
- 3d. Not proposed for change: the action call's trim-versus-sell-all weighing on TSLA (bull-case optionality against a negative base), the evidence cross-checks in gathering (which source states the delivery figure, whether a snippet's period label is FY2025 or Q4 2024) and the one closing check of "one JSON object, no code fence" are the model doing the task.

### Finding 4 — PSX action rationale names the hurdle without quoting the tested returns (v41 check failed on one of two)

- Evidence.
  PSX `action_rationale`: "Maintain the position because while valuation is attractive and base case returns exceed the hurdle rate, low quality metrics … preclude aggressive accumulation."
  The check in the 2026-09-17 list §Verification on attempt 7 reads: no rationale cites a hurdle state word, and the rationales that name the hurdle quote the tested returns.
  TSLA's rationale passed (it quotes −53.5 % and names no hurdle).
- Cause.
  CAPITAL EFFICIENCY renders "the computed twelve-month total return in each scenario … and the hurdle rate it is measured against", so the hurdle is a named quantity in the packet and the model summarises the comparison by name.
- Change proposed.
  The rationale task clause asks for the figures by value: "name the returns you weighed by their values; do not describe them by their relation to another figure".
- Check, on attempt 8.
  Every rationale that mentions the hurdle carries the return figures it tested.
- Stamp: `portfolio-v48`.

### Finding 5 — Investor-relations hosts refuse the fetcher: the primary earnings sources never enter the packet

- Evidence.
  24 of 45 fetch attempts failed across the two stocks; 17 were live failures, 5 host cooldowns and 2 remembered failures (entry 4's memory and entry 6's cause naming both working).
  The failing hosts are the issuers' own: `investor.phillips66.com` (7), `ir.tesla.com` (5), `ir.marathonpetroleum.com` (2), `www.hfsinclair.com` (1) — every one HTTP 403 — plus Reuters 401 and the paywalls (Seeking Alpha, Barron's, Benzinga).
  What succeeded was tier-4 commentary (Motley Fool, Zacks, GuruFocus, Yahoo) and one `sec.gov` page, so TSLA's Q2 figures were persisted from CNBC and Fool rather than the release, and PSX's utilization from a stub 10-K extraction.
- Change proposed, to be ruled between two routes.
  Either the render-first (embedded webview) profile is tried on the first 403 from an issuer IR host before the host enters cooldown, or the app falls back deterministically to EDGAR: an issuer press release that fails resolves to the matching 8-K exhibit 99.1 on `sec.gov`, which the fetcher reaches.
  The EDGAR route is the primary source and needs no browser, so it is the recommendation.
- Check, on attempt 8.
  On a stock whose IR fetch fails, a `sec.gov` release body appears in the same pass's evidence and the claim cites it.
- Stamp: none for the fetch route; `portfolio-v48` only if the tool-result text changes.

### Finding 6 — Distillation output nearly doubled on a third of the topics

- Evidence.
  Distillation completion tokens: attempt 6 TSLA 2,961 and PSX 2,521 over six and seven topics; attempt 7 TSLA 5,445 and PSX 4,547 over two topics — roughly 500 tokens per topic then, 2,500 now — at 34 tokens per second of decode, so 197 s and 168 s against 106 s and 96 s.
  Prompt sizes are comparable (62,778 and 65,162 characters against 54,299 and 68,619).
- Cause, candidate.
  Entry 3 (`d7fa5dd`) gave every persisted claim `retrieved_at`, a `publication` object and a `fact_period` object; `retrieved_at` is app-supplied from the evidence ledger, and the plan slice reads whether the model now emits the other two per claim, and whether the claim count per topic also rose under the three-pass structure of Finding 2.
- Change proposed.
  Any date the app already holds for a cited page (retrieval time, reported publication) is carried by the app through `evidence_ref` and removed from the model's output shape; the model emits only what the source text states (the fact period, and a publication date only where the app has none).
- Check, on attempt 8.
  Distillation `eval_count` per topic returns toward the attempt-6 band with the entry-3 fields still fully populated on the persisted claims.
- Stamp: `portfolio-v48`; `checkpoint-v15` only if the persisted claim shape changes.

## Observed, no change proposed

- CFTC positioning: all eight contracts `unavailable` for the whole run; `publicreporting.cftc.gov` answered HTTP 503 "Site Currently Unavailable" from the host at bring-up, so the packet carried no positioning (attempt 6 carried all eight).
  Fail-soft as designed; re-read on attempt 8.
- All 33 stock rows arrived with an empty issuer description, leaving the listing-resolution guard unverifiable on every stock (the standing ingestion watch).
- Two PSX pages were truncated at the 12,000-character fetch cap and the eight-turn gathering cap was hit five times across the two stocks; both bind more when the budget is spent on follow-ups and are re-read after Findings 1 and 2.
- TSLA's ledger audit rejected the model's claim that gross margin had already broken its 18 % floor (trailing 18.85 %; the model read the quarterly 16.8 %) — the guard working; the debt-to-equity margin renders as "±0.001" for a stored 0.0005, a display-rounding watch.
- The action trace on TSLA notes the analysis date as "future date noted in prompt context" once; no rationale or claim carried it.

## Run configuration and boundaries

- Ollama v0.32.5, one slot, flash attention, fresh daemon and serve log for this run (`ollama.log`); runner `n_ctx` 131072; the sampler chain and sampler params blocks per task launch are in the log for the entry-8 capture.
- SearXNG pinned image with the Serper key rendered; bring-up probe: serper 10, google 10, google cse 20, reuters 9, bing 7; qwant and duckduckgo CAPTCHA; mojeek 0.
- Store after the cancel: TSLA and PSX checkpoint rows, `portfolio_runs` 0, `job_runs` max id 7, `web_source_state` 27 host rows, `web_documents` 21, `portfolio_research_seeds` 4; the prod store untouched.
  Attempt 8 re-wipes first.
- Whole-run boundaries for entry 11's rates: `job_runs` 7 `started_at` 2026-09-19T21:46:41Z, `finished_at` 2026-09-19T23:05:31Z; both holdings' `ResearchAuditRecord.elapsed_secs` are the 30-minute wall.
