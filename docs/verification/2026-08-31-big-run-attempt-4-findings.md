# Big confirmation run — findings (2026-08-31, attempt 4, run id 5)

Findings from the second launch of the single big confirmation run — the
`portfolio-v32` debut, from a wiped store so every holding is a debut, across the
same 47-position book.
This is attempt 4: attempts 1 and 2 failed in the since-removed construction
stage, and attempt 3 (2026-08-30) was cancelled by the user at 2 of 47 when the
keyless SearXNG engines rate-limited under the research loop and spilled to the
Tavily fallback.
The watch set it reads against is `big-run-watch-set.md`; the attempt-3 record it
carries forward from is `2026-08-30-big-run-findings.md`.

This record accumulates findings during and after the run, so some entries are
interim.
It keeps attempt 3's thematic finding numbers, so a reader can compare a finding
across runs — Finding 1 is the search backend, Finding 2 the ledger `quant`
population, Finding 3 the action call, and so on — and each entry states whether
it reproduces, does not reproduce, or is resolved relative to attempt 3.
Where a finding rests on a single holding it is labelled as such and carries a
quantify-across-the-book action rather than a conclusion; a single holding is not
a rate.
Nothing here was changed while the run was in flight — every fix is deferred to
after the run, so adoption can read the run's own measured rates rather than a
one-holding impression.

## Run configuration

Dev app (`cargo run`, dev-scoped store) with the production corpus imported for
report continuity — 30 reports, 67 vector rows (38 learnings, 29 summaries), 14
baseline snapshots — and the portfolio store wiped to a clean debut
(`prior_run_id = None`, zero prior runs, checkpoints, episodes, or quick-checks).
The reasoner is `qwen3.5:122b-a10b` on the M5, 100 % GPU, `num_ctx = 131072`, with
`OLLAMA_FLASH_ATTENTION=1` and `OLLAMA_NUM_PARALLEL=1`.
Web research runs SearXNG-only (the Tavily fallback was removed whole after
attempt 3), now with a Serper.dev paid Google SERP wired as a SearXNG
`json_engine` — the reliable floor immune to the egress-IP blocks the keyless
scraped engines suffer — alongside a widened keyless engine set kept as zero-cost
redundancy.
Header stamps observed on the checkpoint header: `portfolio-v32`, `checkpoint-v8`,
`evidence-floor-v4`, `grade-v2.3` — the shapes the watch set requires.
Run identity: progress id `61bf9b21-9b13-4400-bb04-0e5ca2534d42`, portfolio run_id
`b47c7e23-34de-414e-8bb0-89e909a25a97`; `job_runs` id 5 is written at the run's
terminal state.

## Finding 1 — Search backend under the research loop: attempt 3's failure mode is closed

The attempt-3 failure mode — keyless engines blocked, SearXNG empty, queries
spilling to the Tavily fallback whose quota is reserved for the report job —
cannot recur, because the Tavily fallback was removed whole; a blocked SearXNG now
degrades to thinner research rather than spending the report job's quota.
The bring-up probe and the first in-run holding both confirm the backend serves.
At the pre-run per-engine probe, Serper answered with real organic URLs, and
`google`, `bing`, `qwant`, and `reuters` each served, while `google cse`
(suspended), `duckduckgo` (CAPTCHA), and `mojeek` (empty) were blocked but
fail-soft — the same volatile-block pattern attempt 3 documented, now with the
Serper floor underneath it.
In-run, TSLA's research loop fired multiple `web research/search` calls each
returning 12 hits and `web research/fetch` calls extracting cleanly
(19,964 / 20,588 / 6,647 / 3,328 characters on the fetches sampled), so the loop
reached the model with real evidence rather than an empty packet.

The re-attempt gate the watch set required is therefore met, and the spillover
that ended attempt 3 is closed by construction.
What remains open is the serve-rate-under-full-volume question — whether the
Serper floor plus the keyless set hold their hit rate across ~1,000–1,500 queries
per run — which only the completed run can answer, and which informs the still-
provisional permanent engine set.

## Finding 2 — Ledger `quant` population: does not reproduce on TSLA

Attempt 3 found TSLA authoring numeric gross-margin falsifiers into statement
prose while leaving the machine-evaluable `quant` object null, degrading those
conditions to qualitative on run 2.
On this run's TSLA the same worry surfaced in the reasoning trace — the model said
the prompt "doesn't explicitly list a JSON schema for individual ledger entries"
and that it would "infer standard structure" — but the persisted output does not
reproduce the under-population.
TSLA's `verdict.thesis_ledger` decoded into four structured conditions of which
three carry a fully populated `quant`: `gross-margin below 0.15 ± 0.02`,
`price above 460.0 ± 0.25`, and `revenue-growth below 0.1 ± 0.02`.
The fourth condition is a `falsifier` with `quant` null and `technology_class`
true, and its statement is a qualitative regulatory event —
"NHTSA Engineering Analysis concludes that software-only updates are insufficient
to address identified safety failure modes in FSD v13+" — which has no
machine-evaluable series, so a null `quant` is the intended shape rather than
under-population.

The reason the trace's structural anxiety was harmless is that the emitted object
is grammar-constrained: the model's inferred `type` field was forced to the real
schema key `role` (falsifier/trigger), and the schema's other keys
(`condition_id`, `trigger_family`, `tripped`, `eval_state`, `supersedes`) were
supplied by the grammar regardless of what the thinking resolved to.
This is one holding, not a rate, and the attempt-3 pattern remains the thing to
count across the book: over every completed holding, the conditions whose
statement carries a numeric threshold while `quant` is null, with funds and
thin-data names the likeliest place the under-population returns.

## Finding 3 — Action-call prompt friction: reproduces, with the clarity cost visible in the trace

The per-holding action call is the sharpest prompt-friction surface, and it
reproduces attempt 3's signal in a fuller form: the reasoning trace reaches a
correct, well-justified conclusion by a process that is not clear but exhaustive.
The persisted output is clean — TSLA persisted `action = trim`
(`action_source = model-chosen`), `conviction = medium`, horizon short-neutral /
mid-bearish / long-bullish, `dead_money = indeterminate`, both engine and model
arms grading F, and a one-sentence rationale that flags the tax cost of realizing
the gain without letting it move the rung.
The rationale persisted is verbatim the trace's final one-sentence draft, so the
thinking and the stored verdict match exactly.

The friction is in the process, not the destination.
Three tells mark it.
First, the trace relitigates the same fork roughly six times with no new
information — Trim, then "Maybe Hold?", then "Let's check Sell-all", then "Stick
with Trim", then "Wait, check Sell-all", then "Okay, Stick with Trim" — re-weighing
the identical facts (F grade, −11% base case, −80% bear, +140% bull, the tax on a
50% unrealized gain) and returning each time to the same rung.
Second, it re-derives settled points repeatedly, re-reading and re-confirming the
"an indeterminate capital-efficiency read is neutral — do not let it tilt the rung
toward selling" rule at least four separate times, each time re-concluding the
same thing.
Third, it closes by fatigue rather than by a decisive cut — "Okay, Stick with
Trim" reads as stopping the loop, not as an argument finally settling it — with
minor numeric slips along the way (a stray "+240%" against the +140% bull, a
"$620 bull" in prose against the persisted targets).

The root cause is the prompt shape, read against `action_system_prompt`
(`pipeline.rs:4185`): a single dense paragraph carries roughly eleven co-equal
considerations behind a trivially small output — the rung plus a one-sentence
rationale, two keys — so the instruction is heavy relative to the task rather than
the task being hard.
The considerations carry no precedence, so the model re-weighs the whole set
combinatorially instead of applying an order, and several are phrased as
prohibitions — "do not let [the indeterminate read] tilt the rung toward selling",
tax is "never the mover", "never a departure note of your own" — which the model
re-audits itself against on each pass rather than executing once.
Two of those rules are also stated twice, once in the system prompt and again in
the user prompt (`action_user_prompt` — the capital-efficiency read at
`pipeline.rs:4281`, the tax posture at `:4239`), which amplifies their salience and
the re-checking.

The sharpener is that the paragraph mixes two layers that want different
treatment.
One layer is reasoning guidance — telling a frontier reasoner to weigh grades,
scores, conviction, horizon, and implied moves — which the model would do unasked,
and which is the cuttable bloat.
The other layer is behavioral contract — tunnel-vision isolation, the
indeterminate-capital-efficiency-is-neutral rule, tax-as-flag-not-mover, and the
no-fabricated-price rule for role/risk-only vehicles — each of which encodes a
specific ruling and is unenforceable by the schema, because a schema binds
structure (two keys, an enum rung, one sentence) but not the semantics of how the
rung was derived, and the two-arm design deliberately does not validate the model
arm against the engine (ruled 2026-08-29, Codex I6), so the prompt is the only
place these contracts live.
On TSLA every one of them was honored — isolation held, the indeterminate read
stayed neutral, tax was flagged not moved — so they are doing real work, and
attempt 3 already saw the indeterminate read lean toward selling even with the rule
present, so weaker enforcement would regress rather than clean up.
So the cost is not correctness but time: this much deliberation per holding is a
real component of the per-holding wall clock, which makes Finding 3 also a
throughput finding.

The post-run candidate follows from the split: cut the reasoning-guidance layer to
the shape the data already implies — here is the verdict evidence, here are the
values to determine (the rung and its one-sentence rationale), here is the schema —
and keep the behavioral contracts, but as a short ordered list of gates stated
once, not woven into a how-to-weigh narrative and not duplicated across the two
prompts.
The order that falls out is grade and target evidence first, then risk tier, then
investor profile as the tie-breaker, tax as a required flag, and capital efficiency
explicitly out of the sell decision unless the read is `fails`.
That is close to a minimal data-plus-schema prompt, minus the four-or-five hard
constraints the schema cannot carry — the tempting "strip the how-to-think prose
entirely" move would drop those with the bloat and silently re-open the rulings
they encode, since a stripped output stays schema-valid while deriving the rung the
wrong way, and nothing downstream catches it.
The change should collapse the branch-by-branch self-checking to a short ordered
deduction, cut per-holding time, and subsume attempt 3's two narrower signals (the
engine-set / departure-annotation re-reading and the indeterminate-capital-
efficiency lean) into one fix.
It rests on one holding; the quantify-across-the-book action is to note, per
holding, whether the action trace oscillates across rungs before settling, so the
friction is a measured pattern rather than a one-holding impression before the
prompt is touched.

### Cross-cutting note — thinking is a free side-channel

Findings 2 and 3 both come from the model's thinking block, which is an ephemeral
side-channel: the grammar-constrained `format` applies only after the thinking
token, and only the resulting JSON is persisted and validated, so a meandering or
uncertain trace cannot by itself produce malformed or invalid output.
Both were confirmed against the persisted TSLA verdict — the ledger `quant` came
through populated and the action and rationale matched the trace — so the signals
worth acting on are prompt-clarity and throughput signals for post-run
calibration, not correctness bugs.

## Finding 4 — The 6d research-findings turn is fragile; it claimed the first hard holding failure

This is the run's dominant retry story and it escalated past the single TSLA retry
first observed.
Across the first seven holdings the 6d research-findings turn's `SchemaParse`
failure fired on five of them — TSLA, ARKF, DIA, PGNY, and NFLX — with only PSX and
SPMO clean, a fired-retry rate around 70% of holdings on this one turn, six retries
total.
All share one cause: the terminal findings turn returned content that failed to
deserialize into `FindingsWire`, the error consistently
"expected value at line 1 column 1" — an empty or non-JSON body rather than a
truncated one, which points at an empty completion or the grammar not engaging on
the terminal turn rather than at truncation, narrowing attempt 3's open question in
that direction.

Most of the five recovered on the one bounded retry and completed with full
verdicts.
PGNY did not: both its first attempt and its retry returned the empty body, so the
findings turn failed after its one retry — and that is a hard model failure, not a
fail-soft degrade.
The fail-soft posture in `research.rs` covers web errors — an errored search or
fetch degrades the evidence — but the model's terminal findings-synthesis call is
not web evidence: a findings parse that fails after its one retry returns `Err`
(`research.rs:1108–1132`), which propagates through `pipeline.rs:304`'s
`.context("researching the holding")?` and fails the holding.
This corrects attempt 3's Finding 4, which stated a findings turn failing after its
retry "degrades that pass to a thinner packet rather than failing the holding" —
the code returns `Err` and fails the holding, and PGNY demonstrates it live.

The consequence is contained, and it is the first live exercise of the 2026-08-31
per-holding failure-isolation slice: PGNY was isolated as a failed holding — absent
from the five persisted completed holdings, the run continuing on to NFLX rather
than failing — exactly the posture the slice built (a hard per-holding failure no
longer fails the run;
`2026-08-31-portfolio-failure-isolation.md`).
So the finding has two halves: the failure-isolation slice works as designed on its
first real failure, and the 6d terminal turn is fragile enough that at a ~70%
per-turn retry rate a double failure is a matter of when, not whether — the
post-run candidate is to investigate why the terminal findings turn returns empty
bodies (correlating the `SchemaParse` retries against the data-health truncation
and context-pressure flags, the tools-plus-`format` grammar-engagement edge the M5
pre-flight could not reproduce at 8/8, and whether the forced-terminal
tools-withheld turn changes the grammar path).
The per-holding fired-retry rate and the recover-versus-fail split come from the
persisted `model_retries` telemetry across the completed run.

## Metrics observed — interim, single-holding

The run is in flight (seven holdings attempted — TSLA, PSX, SPMO, ARKF, DIA
completed, PGNY failed, NFLX running — at the time of writing), so these are
observations, not rates.

- Search backend: Serper plus `google` / `bing` / `qwant` / `reuters` serving at
  the probe; TSLA's in-run searches returned 12 hits each with clean fetches; no
  Tavily spillover is possible (fallback removed).
- Ledger `quant` population: TSLA authored three of four conditions with a fully
  populated `quant` and the fourth correctly null on a qualitative regulatory
  falsifier (Finding 2) — the attempt-3 under-population does not reproduce here.
- Action call: TSLA persisted `trim` (`model-chosen`), the trace matching the
  stored verdict, reached by an oscillating process (Finding 3).
- Model-retry: six retries across five holdings (TSLA, ARKF, DIA, PGNY×2, NFLX),
  all on the 6d research-findings turn's `SchemaParse` cause; four recovered, PGNY
  double-failed (Finding 4).
- Failed holdings: PGNY (1 of 7 attempted) — the research findings turn failed
  after its one retry; isolated as a failed holding, the run continuing (the
  failure-isolation slice's first live exercise).

A future full run should recompute these as real rates and add the attempt-3
open items — the serve-rate-under-full-volume for Finding 1 and the
`SchemaParse`-versus-truncation correlation for Finding 4 — that only the
completed run can close.

## Finding 4 — fix B (landed 2026-08-31)

Fix B splits the Step-6c research turn so tools and the findings grammar never share a request: the gathering loop carries the tools with no `format`, and a separate synthesis call authors the pass's findings from a fresh, tool-history-free conversation carrying the grammar and no tools — mirroring the interpretation call, which uses the same clean shape and never fails its parse.
The user ruled the five build decisions (always-synthesize-fresh, reuse the distillation input-budget guard for evidence sizing, a new tailored synthesis prompt dropping the "as JSON" phrasing, keep the bounded retry-once, capture the failing body on a residual parse failure), all recorded via the selector UI on 2026-08-31.

The mechanics: gathering turns pass `tools` only, the synthesis call passes the findings `format` only, and `synthesize_findings` keeps the same two retry legs and four-call bound the tool loop carried; the synthesis brief renders the gathered pages as the only citable evidence, carrying the full source annotation (tier, evidence kinds, extraction quality, recency, thin-stub) and sized against the shared input-budget guard with a water-fill allocator (short pages whole, long pages truncated with an inline marker; a page too small to carry a marker is dropped entirely — its URL never listed *and* excluded from claim validation, so a claim citing it is rejected rather than accepted against evidence the synthesis never saw — and summarized, so no source is rendered as a deceptively-empty page; every truncation or drop is recorded as a gap).
`PROMPT_VERSION` moved to **`portfolio-v33`** so an interrupted pre-fix run cannot resume into the new synthesis contract — the debut stamp the next launch confirms.

Verification: `cargo test` and `cargo clippy --all-targets --all-features` both green; new tests pin the synthesis call's fresh-two-message / tools-XOR-grammar shape, the allocator's fit / overflow / mixed / rendered-length behavior, and the evidence planner's cut-marker and sub-marker-drop boundaries.
Eight Codex review rounds hardened it (source-quality fidelity, the water-fill allocator, marker accounting, and the doc-contract sweep); the record and the `stage_requests_carry_the_per_stage_mode_options_and_residency` / `prompt_version_is_stamped_for_the_model_arm_domain_gate` tests carry the details.
The one thing static review cannot close — whether B eliminates the ~70% empty-body rate on the live 122B — is attempt 5's to confirm.

### Post-landing review (2026-08-31)

A ninth Codex pass over the landed Fix B raised four findings, all verified against the code.
The first fixes closed three boundary-hygiene gaps the tools/grammar split introduced (rather than reworking the split): a fetch that extracted no body is dropped from evidence (gap-recorded, never entering the allow-set); the extracted page title, which the discarded gathering transcript used to carry, now leads each synthesis header; and the gathering phase's degradation (failed or empty searches, failed fetches, budget-skips) is surfaced to the sole findings author as an explicit "GATHERING WAS PARTIAL" note and persisted as a data-health gap, so a partial pass tempers conviction rather than reading as complete.

A tenth pass then found that the first round of fixes was incomplete, and one of its claims here was wrong.
The empty-page fix had kept *title-only* pages (a headline, no body) citable, which reopened the same hole: a page title is untrusted and unbounded, and cache hits spend no fetch budget (`exec_fetch` increments the ceiling only on a live fetch), so the per-pass page count is not bounded by `MAX_FETCHES_PER_HOLDING`, and a burst of title-only cache hits — or one oversized title — could inflate the header framing past the input guard while every body allocated to zero.
The earlier claim in this record that the breach was unreachable was therefore false and has been removed.
The fix requires a non-empty *body* for a page to be citable (a headline alone never makes a URL citable, and the title still leads a kept page's header), caps the title to a headline length, and adds a header-fit trim that drops the trailing overflow entirely so the rendered brief is provably within the guard whatever the page count.
The degradation coverage was also completed — the turn-cap cut-off and malformed/unknown tool calls now feed the same note and gap — while a proposal to hard-override `topic_answered` on a degraded pass was declined: the flag has no control-flow or downstream-digest consumer (it is a persisted annotation only), so forcing it false adds no behavioural value and would wrongly demote a topic genuinely answered despite a single failure; the note tempers the model's own conviction, which is what flows downstream.

Because the synthesis brief's changes alter the synthesis input and so a completed holding's analysis, `PROMPT_VERSION` moves to **`portfolio-v34`** — the resume contract (`job::resume_eligibility`) refuses a resume across the changed synthesis semantics rather than mixing them, and this is now the debut stamp the next launch confirms (superseding the `portfolio-v33` the earlier §Finding 4 named).

A third pass found the input guard still had one hole and the degradation coverage one exit.
The evidence sizing bounded only the headers and bodies, not the pass *prefix* (`pass_brief`) — which is the gathering request's whole user message and the synthesis prefix, and carries the accumulated prior-claims ledger (up to every claim from every prior pass on the disconfirming pass) and the follow-up text, both unbounded model output with no schema length cap — so a large ledger could exceed the guard before any page was sized.
The fix bounds the prefix: each prior claim and the follow-up are capped, the ledger block stops at a total budget with an omitted count, and `pass_brief` takes a final head-cap (preserving the essential framing that leads it) so neither request can exceed the guard whatever the ledger's size.
The degradation coverage was completed for the last exit — a fetch/wall-clock budget exhausted exactly at a turn boundary (where no later in-turn call triggers the mid-turn skip) now sets its own signal, so a forcibly-stopped gathering pass always reaches the synthesis and data-health.
These prefix and degradation changes fold into the same `portfolio-v34` bump (never run, so no separate stamp).

A fourth pass found the last malformed-call path still silent: the decoder accepts arbitrary JSON for `tool_calls`, and `parse_tool_calls` returns an empty vector for any non-array value, so a present-but-non-array `tool_calls` (an object, a stringified array, a scalar — the decoder having already collapsed empty arrays and null to None) was dropped without incrementing `malformed_calls`, warning the synthesis, or recording a gap.
The gathering loop now guards for a non-array `tool_calls` before it echoes the assistant turn: it counts the malformed call and ends gathering, so the malformed turn reaches the synthesis and data-health without pushing an off-protocol assistant message back onto the wire (the reason for the guard rather than the reviewer's suggested inline `Unknown`, which would echo the malformed value).
A recovered stringified-array `tool_calls` is treated as malformed here rather than parsed — the local Ollama path emits native arrays, and the stringified quirk belongs to the cloud tool-use path — which is a bounded, honest degradation, not a silent drop.
This folds into the same `portfolio-v34` work.
New tests pin the body-required drop and title render, the title cap and header-fit guard under a page burst, the prefix bound under a huge ledger, the turn-cap / malformed / budget-exhausted degradation summaries, the degradation note in the brief, and the degradation gap end-to-end (search/fetch failure, exact-ceiling exit, and a malformed non-array `tool_calls`).
