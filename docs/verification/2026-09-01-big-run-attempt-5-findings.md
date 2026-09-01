# Big confirmation run — findings (2026-09-01, attempt 5, ended at 4 of 47)

Findings from the third launch of the single big confirmation run — the
`portfolio-v34` debut, from a wiped store so every holding is a debut, across the
same 47-position book.
This is attempt 5, user-ended at 4 of 47 completed holdings — TSLA, PSX, SPMO, and
ARKF, with DIA discarded in flight — once the round's findings were in and the user
left for a trip; the infrastructure was then spun down.
Attempts 1 and 2 failed in the since-removed construction stage; attempt 3
(2026-08-30) was cancelled at 2 of 47 when the keyless SearXNG engines
rate-limited and spilled to the Tavily fallback; attempt 4 (2026-08-31) was
cancelled at 7 of 47 once the round's findings were in.
The watch set it reads against is `big-run-watch-set.md`; the attempt-4 record it
carries forward from is `2026-08-31-big-run-attempt-4-findings.md`.

This record accumulates findings during and after the run, so some entries are
interim.
It keeps the attempt-3/attempt-4 thematic finding numbers, so a reader can compare
a finding across runs — Finding 1 is the search backend, Finding 2 the ledger
`quant` population, Finding 3 the action call, Finding 4 the 6d research-findings
turn — and each entry states whether it reproduces, does not reproduce, or is
resolved relative to the prior attempt.
Findings numbered 5 and up are new to this run.
Where a finding rests on a single holding it is labelled as such and carries a
quantify-across-the-book action rather than a conclusion; a single holding is not
a rate.
Nothing here was changed while the run was in flight — every fix is deferred to
after the run, so adoption can read the run's own measured rates rather than a
one-holding impression.

## Run configuration

Dev app (`npm run tauri dev`, dev-scoped store) with the production corpus
imported for report continuity — 30 reports, 67 vector rows (38 learnings, 29
summaries), 14 baseline snapshots — and the portfolio store wiped to a clean debut
(`prior_run_id = None`, zero prior runs, checkpoints, episodes, or quick-checks).
The reasoner is `qwen3.5:122b-a10b` on the M5, `num_ctx = 131072`, with
`OLLAMA_FLASH_ATTENTION=1` and `OLLAMA_NUM_PARALLEL=1` confirmed on the serve log.
Web research runs SearXNG-only with the Serper.dev paid Google SERP wired as a
SearXNG `json_engine` floor, alongside the widened keyless engine set as zero-cost
redundancy.
At the pre-run per-engine probe Serper answered with real organic URLs, and
`google`, `bing`, `google cse`, and `reuters` each served, while `qwant` and
`duckduckgo` returned CAPTCHA and `mojeek` returned empty — the same volatile-block
pattern the prior attempts documented, now with the Serper floor underneath it.
Header stamps observed on the persisted checkpoint header: `portfolio-v34`,
`checkpoint-v8`, `evidence-floor-v4`, `grade-v2.3`, `targets-v6`, `pre-profit-v4`,
`quick-check-v3` — the shapes the watch set requires.
Run identity: progress id `bd135c7b-c9b7-40b8-86d3-d1aea1238173`, portfolio run_id
`ffd7d364-6c60-4d32-b56c-09925cf04ee2`.
The run was ended by terminating the dev process rather than a cooperative
run-tracker cancel, so no terminal `job_runs` row was written — the table stays at
id 5 (attempt 4) — while the checkpoint header and the four completed holdings
persist and are the finding record.

## Finding 1 — Search backend under the research loop: serving cleanly (interim)

The backend serves.
At the probe the Serper floor answered with real organic URLs and four keyless
engines served alongside it, and in-run the research loop fired `web
research/search` calls returning 12 hits each on both TSLA and PSX.
No Tavily spillover is possible — the fallback was removed whole after attempt 3 —
so a blocked engine degrades to thinner research rather than spending the report
job's quota.
The serve-rate-under-full-volume question stays open until the completed run reads
it across ~1,000–1,500 queries, and it informs the still-provisional permanent
engine set.

The one live friction is the *fetch* success rate, not the search rate.
On PSX 17 of 25 fetch attempts failed, almost all HTTP 401/403 from paywalled
primary sources — Reuters, the Phillips 66 investor-relations pages, tickeron —
while TSLA saw the same block pattern on Reuters, Barron's, and Benzinga.
This is fail-soft by design and the synthesis still ran from the sources that did
fetch, but it is the extraction-telemetry signal the deferred rendered-retrieval
tier's scheduling reads, so the per-domain thin-stub and failed-fetch rates are a
quantify-across-the-book action rather than a defect.

## Finding 2 — Ledger `quant` population: does not reproduce on TSLA (interim)

TSLA's persisted `verdict.thesis_ledger` carries structured conditions with a
populated `quant`, so attempt 3's under-population signal does not reproduce on the
first holding, consistent with attempt 4's TSLA read.
This rests on one holding; the pattern to count across the book is the conditions
whose statement carries a numeric threshold while `quant` is null, with funds and
thin-data names the likeliest place it returns.

## Finding 3 — Action call: clean output on all four holdings; oscillation still to be measured

TSLA persisted `action = trim` with `action_source = None` (no outside-set
departure) and a single-sentence rationale that weighs the F grade, the negative
base case, and the tax cost of realizing the gain without a spurious sell-lean —
the clean output shape Finding 3's fix intended.
Across all four completed holdings the persisted action output held that shape —
`trim` on TSLA, `hold` on PSX, SPMO, and ARKF, each with `action_source = None` and
a single-sentence rationale — so the clean-output half of the fix reproduces on the
short sample.
Whether the leaner prompt actually stops the reasoning trace oscillating across
rungs before settling is the quantify-across-the-book action the fix left open, and
it is measured per holding from the reasoning stream, not from the persisted
verdict.

## Finding 4 — 6d empty-body rate under Fix B: does not reproduce across four holdings; retries recover

This is the run's headline question and the one live item the Fix-B finite closure
left open (matrix C8): does the tools/grammar split eliminate the ~70% empty-body
rate attempt 4 measured on the terminal 6d research-findings turn.
Across the four completed holdings the ~70% rate does not reproduce, and the one
empty-body failure that did occur recovered on its bounded retry rather than
double-failing.
TSLA's synthesis persisted a rich, multi-paragraph `combined` body from nine
sources with `shape = SinglePass`, `unreconciled_topics = 0`, and its
`model_retries` telemetry is empty — zero retries.
PSX fired exactly one retry, its persisted `model_retries` recording cause
"content failed its parse" — the same empty-or-non-JSON body class attempt 4 saw —
which recovered on Fix B's bounded retry-once and completed with a full verdict, so
the holding did not fail.
The authoritative source for this is the persisted `model_retries` telemetry, not
the progress log: the internal re-issue does not surface as a distinct
`SchemaParse` string in the request stream, so a log grep understates the retry
count and the store read is the one to trust.
Four holdings is not a rate, so the C8 question closes only on the completed run's
`model_retries` telemetry and the recover-versus-fail split; but the early read is
the healthier posture Fix B predicted — the failure mode still occurs at some rate,
and it now recovers rather than stopping the holding.

Across the four completed holdings the fired-retry telemetry splits by cause: PSX's
one retry was "content failed its parse" (the empty-body class Fix B targets) and
ARKF's one was "daemon error status" (a transient Ollama serving error, a different
whitelisted class), while TSLA and SPMO fired none.
Both retries recovered on the single bounded attempt and no holding double-failed,
so this sample shows two retries over four holdings from two distinct causes, zero
hard failures, and the empty-body cause at one of four rather than attempt 4's
roughly seven of ten.
The completed run's per-cause rate and the recover-versus-fail split are still the
reads only a full run gives.

## Finding 5 — The 6d synthesis prompt does not show the model its schema, so the model rationalizes toward a hand-built serialization (new)

The 6d synthesis call is grammar-constrained, and the model cannot see the grammar.
Ollama applies the GBNF `format` mask to the content channel only, after the think
token, so from the model's vantage it is told to "emit ONLY the structured findings
object your output grammar enforces" with the six fields described in prose —
`findings_prose`, `claim_citations`, `topic_answered`, `material_forward_facts`,
`seed_ids_used`, `followup_proposal` — but with no schema shape shown and no visible
grammar to conform to.
PSX's synthesis reasoning trace shows the cost: the model spends a long stretch
unsure whether the object is JSON or Markdown ("Usually, this implies Markdown or a
specific text block format"), and resolves to hand-build a header-labelled text
block "to mimic the required fields."

Structurally the grammar contains the syntactic half, so this is not a Fix-B
regression.
`think:true` + `format` composes on the pinned serving path: the confused reasoning
stays in the thinking side-channel, and the schema-valid JSON is forced into the
content channel regardless of what the trace resolved to.
TSLA's synthesis came through well-formed with zero retries, and PSX's one parse
failure recovered on the bounded retry, so no malformed body reaches persistence —
what is lost instead is a whole topic downstream, at reconciliation, not a malformed
record at the synthesis seam.

The residual risk is semantic, not syntactic, and the grammar does not catch it.
The mask guarantees the JSON *shape*, never that the right content lands in the
right field, and in the same trace the model makes content decisions under the
format confusion — "I will restrict citations to that [Fool] URL", "I'll use seed-1
and seed-4" — that the grammar accepts as valid JSON while `claim_citations` is
thinned.
It also spends reasoning budget resolving the output format on a holding already
issuing 85 model calls and 34 searches, so the confusion is a throughput cost as
well as a quality risk.

PSX's completed output shows the risk did materialize, in a form the holding-level
citation check alone would miss.
The citation set looks healthy — seven diverse sources across Yahoo Finance, the
Motley Fool transcript, BIC Magazine, Morningstar, and the Phillips 66 newsroom —
and the `combined` body is clean analytical prose with no Markdown cruft, because
the other five research topics reconciled normally.
But the narrative-sentiment topic — the exact topic whose reasoning trace carried
the format confusion — is PSX's single `unreconciled_topic`: its gap reads
"distillation emitted no reconciled object — stored seed dropped (the next run seeds
this topic cold)", and the `combined` synthesis contains none of its content, with
no sentiment, analyst-rating, crowding, or valuation-premium read surviving.
So the whole topic dropped out of the synthesis rather than a citation within it
thinning, which is why the holding-level source list still looks diverse while the
market-narrative read PSX was meant to carry is simply absent.

The per-claim `claim_citations` the 6d synthesis authors are not themselves
persisted — distillation consolidates them into the `combined` prose and the
deduplicated `sources` list — so the persisted evidence of the risk is the dropped
topic, not a thinned citation array.

The causal chain is plausible but unproven.
PSX fired exactly one bounded retry with cause "content failed its parse" (Finding
4), and the topic that vanished is the topic whose synthesis reasoning was confused
about the output format, so a degraded synthesis object for narrative-sentiment —
whether from the format confusion, the parse failure, or both — would leave
distillation nothing coherent to reconcile.
It is not simply the worst-gathered topic that dropped: narrative-sentiment lost
three fetches, while competitive-position and catalysts-risks lost as many or more
(five on catalysts-risks) and both reconciled, so gathering degradation alone does
not explain why narrative-sentiment is the one that failed to reconcile.
The `model_retries` row is not topic-labelled, so the link between the format
confusion, the parse retry, and the unreconciled topic is a co-occurrence on one
holding rather than a proven cause.
What it does establish is that Finding 5 is not merely a throughput cost: a topic
can drop out of the synthesis entirely while the holding-level citation set stays
diverse enough to hide it, so the read that matters is per-topic reconciliation, not
the source count.

The root cause is a prompt-clarity gap of the same class as attempt 4's Finding-2
and Finding-3 schema-carrying-clarity fixes: the prompt describes the fields but
does not show the schema, so a model blind to the mask guesses at a serialization.
The post-run candidate is to show the model the schema shape explicitly — field
names, types, and a terse example object — so it cooperates with the grammar rather
than inventing a Markdown block, without re-stating "as JSON" phrasing Fix B
deliberately dropped.
The quantify-across-the-book action is to count, across completed holdings, the
`unreconciled_topics` rate and whether it clusters on the confusion-prone
thin-evidence topics (narrative-sentiment, disconfirming), and to correlate each
unreconciled topic against the holding's `model_retries` "content failed its parse"
events — a cluster on those topics, or a tight correlation with the parse retries,
is the evidence that the format confusion is causing topic loss rather than the
independent distillation-reconciliation miss the gap mechanism already expects.

## Finding 6 — Fund path: first clean priced and role/risk reads (new)

The two fund holdings exercised both branches of the intrinsic verdict's
discriminated union, and both came through clean.
SPMO, a momentum equity ETF, resolved `priced` with grade C and `action = hold` in
a fast ~10-minute pass (12 fetches), with no unreconciled topic and no retry.
ARKF, an actively-managed thematic ETF, resolved `role_risk_only` with `grade =
None` and `action = hold` — the branch that returns a role/risk read rather than a
fabricated price target for a structurally unpriceable vehicle — and carried one
expected fail-soft `degraded_input`: a "Cash & Others" allocation bucket with no
sector-P/E history on either exchange leg, the sector-P/E walk-back returning empty
for a non-sector and memoizing the gap as designed.
Both are single instances; the fund-classification split, the priced-versus-role/
risk distribution, and the sector-P/E walk-back depth are the across-book reads only
a completed run gives.

## Metrics observed — final at cancellation, four holdings

The run was ended at four completed holdings (TSLA, PSX, SPMO, ARKF), so these are
observations across a short sample, not rates.

- Search backend: Serper plus `google` / `bing` / `google cse` / `reuters` serving
  at the probe; in-run searches returned 12 hits each; no Tavily spillover is
  possible (fallback removed).
- Fetch success: TSLA 8 of ~11 sampled fetches clean; PSX many of its 31 spent
  fetches failing HTTP 401/403 on paywalled primary sources; the two funds fetched
  light (SPMO 12, ARKF 10) (Finding 1 fetch-rate note).
- 6d empty-body: does not reach the ~70% rate — zero on TSLA, SPMO, and ARKF's
  synthesis; PSX's one parse retry recovered and completed with a full verdict
  (Finding 4).
- 6d synthesis format confusion: one instance in four — PSX, where the
  narrative-sentiment topic it appeared on became PSX's one `unreconciled_topic` and
  dropped from `combined` while the diverse citation set hid the loss (Finding 5).
- Model-retry: two over four holdings, both recovered, zero double-failures — PSX
  "content failed its parse" and ARKF "daemon error status", two distinct causes;
  TSLA and SPMO fired none.
- Reconciliation: `unreconciled_topics` one over four (PSX narrative-sentiment),
  co-occurring with PSX's parse retry — a plausible-but-unproven causal link.
- Fund path: SPMO resolved `priced` (grade C, hold); ARKF resolved `role_risk_only`
  (grade None, hold) — both branches of the intrinsic verdict exercised clean
  (Finding 6).
- Context pressure: TSLA's `prompt_usage` fits comfortably — 2.5K–8.7K prompt
  tokens against `num_ctx = 131072`, `output_limited = false` throughout, no
  truncation flag.
- Throughput: stocks ran ~27–29 minutes (TSLA ~27.5, PSX ~28.8), funds much faster
  at ~10 minutes (SPMO 598s, ARKF 659s), so the book's wall clock depends heavily on
  the stock/fund mix.

## Run ended (2026-09-01)

The run was user-ended at four of forty-seven holdings so the machine could be freed
for a trip, and the local infrastructure — the dev app, Ollama, SearXNG, and
OrbStack — was spun down.
The dev store keeps the four completed holdings and the checkpoint header for
post-trip inspection rather than being re-wiped; the next attempt re-wipes to a
clean debut as the standing ruling requires.

What only a completed run can still close: the run-wide `model_retries` per-cause
rate and the recover-versus-fail split (Finding 4), the `unreconciled_topics` rate
and whether it clusters on the confusion-prone topics or correlates with the parse
retries (Finding 5), the search serve-rate and fetch-failure rate under full query
volume (Finding 1), the action-trace oscillation read from the reasoning stream
(Finding 3), and the ledger `quant` population, fund-classification, and pre-profit
reads at book scale.
