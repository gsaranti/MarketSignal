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

### Final full-sweep corrections before the v34 debut (2026-08-31)

A fifth full sweep found two remaining production boundaries and one stale living-doc contract; all three were fixed before any `portfolio-v34` run, so they fold into the same debut stamp rather than creating another version.
First, the gathering conversation was still aggregate-unbounded: the 12,000-character cap applied to each fetched page, but every assistant tool request and result accumulated across up to eight turns, cache hits did not spend the live-fetch ceiling, and one model response could contain an arbitrary number of calls.
The loop now accepts at most eight tool calls from one turn, executes the deterministic head, records the omitted tail, and moves directly to synthesis.
It also serializes the complete growing message history plus tool schema before every gathering request and before retaining each assistant/tool message, keeping a fixed envelope reserve under the shared interpretation input budget; the first addition that would cross the bound ends gathering, records any executed-but-unretained result and unexecuted calls, and still synthesizes from every fetched page that landed.
Untrusted search and fetch metadata fields are capped or omitted in the gathering render so one title, snippet, date, or redirect URL cannot independently consume the packet.

Second, the synthesis allocator reserved headers for every body-bearing page before it decided which bodies could survive.
Under a sufficiently large cache-hit burst, those soon-to-be-dropped headers could leave every page with less than a truncation marker, drop every source, and return a nearly empty packet despite substantial unused capacity once the dropped headers disappeared.
Selection and water-filling are now one plan: a source is selected only when its header, fixed markers, and at least one usable body character fit; headers for omitted pages are reclaimed first; later compact sources may survive an individually oversized earlier source; and the remaining body budget is water-filled only across the selected set.
The validator allow-set still contains exactly the rendered URLs, and all omissions and truncations remain inline and gap-recorded.

Third, the forward-looking big-run watch set still named `portfolio-v32` for the run and observation admission even though production and resume compatibility were already pinned to `portfolio-v34`.
The watch set now names `portfolio-v34`, distinguishes the Finding-3 rule's v32 introduction from the current attempt stamp, and adds the new per-turn, aggregate-history, and body-bearing evidence watches.

Focused Rust regressions pin the oversized-batch cutoff, the multi-turn cached-page history boundary with every issued packet below the guard, bounded untrusted metadata, the individually unrenderable-source allow-set exclusion, and the 600-page header-reclamation case retaining substantial usable evidence within budget.
Claude Code's follow-up review raised one worthwhile defense-in-depth observation: the selected-page render loop relied on a debug-only assertion that no selected plan was dropped, so a future allocator regression could admit a body-less URL in release mode despite the current proof.
The render boundary now also fails closed in release: an unexpectedly dropped plan is rejected before its URL enters the claim-validator allow-set, counted in a persisted internal-allocation gap, and omitted without adding unreserved text to the synthesis packet; a direct regression pins both the dropped and usable admission cases, and this unreachable-under-current-math safeguard does not change the `portfolio-v34` prompt contract.
Final verification after the corrections: `cargo test` passed 1,458 tests across the library and integration suites with 31 live smokes ignored; `cargo clippy --all-targets --all-features` completed warning-free; `npm run build` passed; and `npm test` passed 46 pure-module tests plus 254 component tests.

### Finite closure matrix after the sixth review (2026-08-31)

The sixth full review withdrew the earlier static approval and replaced the open-ended correction loop with the finite acceptance matrix below.
Its scope starts at the original live failure, not at Fix B's latest diff: attempt 4 combined a growing tool-call conversation with the findings grammar on one terminal request, and five of the first seven holdings returned an empty or otherwise non-JSON findings body on that turn.
The short M5 pre-flight's 8/8 clean result means the repository does **not** prove that tools and `format` are universally incompatible; the production evidence isolates the joint condition of the long-lived tool history, the terminal protocol switch, the model/server serving state, and the grammar constraint, but does not identify which component caused the empty body.
Fix B is therefore a protocol-isolation repair: gathering and structured synthesis are different responsibilities with different output protocols, so each gets its own bounded request shape.
Static closure can prove that the risky joint condition is absent and that every failure or degradation around the new boundary is handled honestly; only the next live 122B run can measure whether the observed empty-body rate actually disappears.

| ID | Acceptance condition | Required proof |
| --- | --- | --- |
| C1 | A gathering request carries tools and no findings grammar; a findings request carries the grammar and no tools or tool-call history. | A production-seam request-shape test plus a pass-loop test over a fresh two-message synthesis conversation. |
| C2 | No gathering or synthesis request relies on daemon-side front truncation. | Aggregate serialized message-and-tool sizing before every gathering issue and retained turn/result; synthesis prefix, headers, markers, and bodies fit the shared input guard. |
| C3 | Empty, non-JSON, structurally incomplete, or semantically blank findings output cannot become a completed pass. | Missing required keys, wrong types, a blank `findings`, and blank claim fields classify `SchemaParse`, exercise the same bounded re-issue, and fail hard only after that bound. |
| C4 | A source is citable if and only if usable body text was actually rendered for it. | The release admission boundary independently rejects `dropped` and zero-text plans; tests construct those states directly rather than relying on current allocator output. |
| C5 | Every partial-gathering, evidence-omission, evidence-truncation, and internal-allocation event on a completed holding survives beyond the model prompt. | The event remains in the persisted per-holding research audit and contributes typed counts to the run-level `DataHealth` summary the Portfolio roll-up renders; no gap-string matching. |
| C6 | Runtime context telemetry measures the variable material presented to the model, including the tool protocol. | One shared serializer counts message roles/content/tool calls plus the tool schema for both the gathering guard and `PromptUsage`; a tool-heavy regression proves it exceeds visible message content alone. |
| C7 | The repaired semantics cannot mix with an interrupted pre-fix holding. | `portfolio-v34` remains the never-run debut stamp and the resume gate refuses a differing prompt version; the Data Health schema is written strictly under the wiped-store pre-release posture. |
| C8 | Static closure and operational confirmation are reported separately. | Full Rust tests, warning-free clippy, frontend build/tests, and diff hygiene close C1-C7; attempt 5 from a wiped store alone closes the live empty-body-rate question. |

No later review may add an unstated acceptance condition and still call itself this closure pass.
A newly discovered issue outside C1-C8 must be recorded separately rather than silently restarting Fix B's approval loop.

### Finite closure result (2026-09-01)

The finite static closure is complete: C1-C7 are closed against production paths, persisted state, the rendered Portfolio surface, and direct boundary regressions rather than review assertions.
C8 remains split by design: its static side is closed, while its live operational side is the one explicitly open Attempt-5 question and was not exercised in this closure.

| ID | Result | Evidence |
| --- | --- | --- |
| C1 | Closed | `pipeline::research_turn_request` is pinned at the production request seam, and `research::the_synthesis_call_is_a_fresh_two_message_conversation_with_the_grammar_and_no_tools` proves every pass uses tools XOR grammar and a fresh two-message synthesis conversation. |
| C2 | Closed | The gathering loop and `PromptUsage` share `local_model::prompt_material_chars`, including roles, content, assistant tool calls, and the tool schema; the existing aggregate-history, per-turn-cap, synthesis-prefix, header-reclamation, omission, and truncation regressions keep every issued packet under the input guard. |
| C3 | Closed | The Rust findings wire requires `findings`, `claims`, and `topic_answered`; `parse_findings_wire` also rejects blank findings and blank claim text/URLs as `SchemaParse`; direct missing-key/type/blank tests, the parse-leg re-issue test, and the four-call hard-ceiling test pin bounded recovery and terminal failure. |
| C4 | Closed | Empty extracted bodies are excluded before allocation, and the release `admit_planned_source` boundary independently rejects both `dropped` and zero-text plans before URL admission; direct tests construct all three states. |
| C5 | Closed | Completed holdings persist the research audit's typed `gaps`; `build_data_health` folds those rows into required `research_degraded_holdings` and `research_gap_count` fields without gap-string matching, adds the visible summary, and the mounted Portfolio regression renders it without misclassifying fail-soft research as an attention alert. |
| C6 | Closed | Gathering guards and runtime telemetry use the same serialized prompt-material projection, and tool-heavy regressions prove roles, tool calls, and the tool schema cannot disappear behind visible-content-only counting. |
| C7 | Closed | `portfolio-v34` remains the pinned debut stamp; the real resume gate's version-drift regression refuses a differing prompt/schema version; the new app-written Data Health fields are required under the wiped-store pre-release posture. |
| C8 | Static side closed; live side open | Independent gates passed: 1,462 Rust tests with 31 live smokes ignored, warning-free `cargo clippy --all-targets --all-features`, `npm run build`, 46 pure-module tests, 255 component tests, and `git diff --check`; no live Portfolio run was launched. |

The closure also corrects the causal claim around Fix B.
Attempt 4 proves a failure of the combined production condition—growing tool-call history, a terminal switch to structured synthesis, the grammar constraint, and the contemporaneous model/server state—but the clean 8/8 short pre-flight does not isolate one of those ingredients or establish a universal Ollama tools-plus-`format` defect.
Fix B is justified independently of that unresolved mechanism because gathering and synthesis have different responsibilities, input lifecycles, and output protocols; separating them removes the observed joint condition, makes each request finitely sizeable, and gives the structured boundary its own bounded parse-and-validation policy.
The next wiped-store Attempt 5 may confirm or falsify the operational prediction that this eliminates the live empty-body rate, but it cannot reopen C1-C7 without a newly identified defect outside this matrix.

### Post-closure C5 completion and degradation-note posture (2026-09-01)

A review after the closure result found C5's "every evidence-truncation event" claim held for budget-forced truncations but not for the per-page fetch cap.
A fetched page over the 12,000-character `PAGE_TEXT_CAP_CHARS` was truncated on store and surfaced only as an inline model marker, and neither synthesis render path — the fits-whole path nor the water-fill allocator — recorded a research gap for it.
So a completed holding whose sole degradation was one truncated long page persisted zero research gaps and read clean in Data Health, which the C5 result row above did not cover.

The fix records the event once at the fetch site from the original extracted length — strictly greater than the cap, cache hits included — as a `fetch_cap_truncations` degradation.
It folds through the existing `PassDegradation` summary and `build_data_health` into the required typed `research_degraded_holdings` and `research_gap_count` fields, so C5 now holds for every evidence-truncation event rather than only the budget-forced ones.

A user ruling on 2026-09-01 then made the synthesis author's degradation note purely factual for every degradation type: it states "GATHERING WAS PARTIAL: <losses> — treat coverage as incomplete" and no longer tells the model to "temper conviction" or to "not mark the topic fully answered".
The persisted gap drops the same prescription, so the audit records the coverage fact rather than a posture.
This revisits the Finding-2 note that had prescribed the tempering: the degradation still records to Data Health, but the model is handed the fact and weighs it itself, consistent with Finding 3's reading that prescriptive prompt language makes the model re-audit itself.

`PROMPT_VERSION` stays `portfolio-v34`, since v34 has never run and every pre-debut change folds into the debut stamp.
Verification: `cargo test` 1,463 passed with 31 live smokes ignored, warning-free clippy, frontend untouched; landed `eea56b2`.
