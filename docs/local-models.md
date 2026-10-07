# Local Analysis Models

The **local analysis suite** — Portfolio Analysis ([portfolio-analysis.md](portfolio-analysis.md)) and Trade Opportunities ([trade-opportunities.md](trade-opportunities.md)) — runs entirely on local open-weight models on the user's machine.
This is a deliberate boundary: the Market Signal Report uses the user-configurable cloud agents (OpenAI/Anthropic — see [agents.md](agents.md)), while the analysis suite is **local-only, keyless, and cost-free at the model layer**.
The two halves share the app's deterministic spine but not their model providers.

This document covers the substrate both local features build on: the serving runtime, the model roster and how work is routed across it, the adapter seam, schema-constrained output, the context-memory discipline that governs multi-call stages, and the run-history continuity pattern.
The per-feature pipelines live in their own documents.

## Serving runtime

Local models are served by a single **Ollama** daemon running on the Apple-Silicon GPU via its **MLX backend where the model architecture is supported, with a llama.cpp Metal/GGUF fallback otherwise**; the Rust backend calls the daemon's **native HTTP surface** — `/api/chat` for generation, `/api/tags` for the supervision/connectivity probes — with `reqwest::blocking` behind the same `spawn_blocking` seam used for the cloud agents (see [agents.md](agents.md)).
Ollama is the chosen runtime because it is the one that, by default, keeps **several models resident simultaneously** and queues/evicts them by memory pressure — the posture the "route each role to the best model" design needs — while the MLX backend gives the GPU throughput where it supports the model (the default 122B reasoner currently lands on the Metal/GGUF fallback — see [local-model-operations.md](local-model-operations.md)).

**Rationale (Ollama over alternatives):**
- multi-model residency and on-demand swap are the default, not an opt-in;
- one native daemon surface (`/api/chat`, `/api/tags`), reachable from the existing reqwest adapter;
- MLX backend closes the historical Apple-Silicon speed gap where a model's architecture is MLX-supported (llama.cpp Metal fallback otherwise).

**Schema-constrained output uses Ollama's native `/api/chat` `format` parameter, not the `/v1/` OpenAI-compatible path.**
The daemon also exposes an OpenAI-compatible `/v1/` layer, but the suite deliberately does not use it: the `/v1/` layer advertises only JSON mode, while reliable JSON-Schema conformance comes from passing the schema to the native endpoint (see [§Schema-constrained output](#schema-constrained-output)).
The native transport also carries its own streaming envelope — newline-delimited JSON rather than the cloud adapters' SSE — so the adapter has its own stream decoder rather than reusing the cloud one.

**Lifecycle.**
The app gates on the daemon in two layers, mirroring the cloud report's *gate on credential **presence**, not connectivity* posture ([scheduling.md](scheduling.md)).
**Presence** of the config values — the Ollama endpoint and the roster ids (the 122B reasoner, which every local job needs; the fast tier is optional) — is known synchronously and gates *proactively*: if a job's required value is unset, its Run button is locked and a persistent **local models not configured** warning shows, cleared the instant the values are filled (exactly like the cloud model selectors / data-source tokens).
**Connectivity** — the daemon actually reachable and the rostered models actually pulled — is *not* probed at startup or on a timer; it is verified only at the **run-gate** (the job's Step-1 precondition check) and by a **manual *Test Connection*** in Settings.
A run-gate connectivity failure blocks that attempt **inline** (pointing to Settings → *Test Connection* to diagnose), not as a persistent warning — except an **engine-only local path that makes no model call** (ATO's Quick Audit, Portfolio's Quick check) skips this daemon-connectivity check, so it runs even when the daemon is configured-but-down ([trade-opportunities.md §Failure posture](trade-opportunities.md#failure-posture), [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)).
The deliberate consequence — accepted to drop the startup probe and any polling — is that a *config-set-but-daemon-down* state shows no proactive signal: on re-open the user discovers it only by clicking Run or testing, the same trade the cloud report already makes (a present-but-unreachable provider fails the run rather than pre-warning).
Both daemon layers are independent of the cloud-report execution gate — a machine with no OpenAI / Anthropic keys can still run the local suite, and a report run needs no local-model configuration (the FMP / FRED data credentials are shared preconditions of both gates; see [configuration.md](configuration.md), [interface.md §Connection status](interface.md#connection-status-local-suite)).

**Provisioning.**
The user installs Ollama and pulls the roster; the app bundles neither.
The unavoidable friction is the **model download** — the 122B reasoner is tens of GB — and Ollama already owns that step best (`ollama pull`, with progress, resume, and dedup), so bundling the daemon would not remove it while adding binary weight and a signing/update burden (the same flat-footprint stance the web tool keeps — see [web-research.md](web-research.md)).
The app instead makes setup turnkey *around* a user-installed Ollama: when a connectivity check (a manual *Test Connection*, or the run-gate) finds the daemon unreachable or a rostered model not pulled, the Settings connection row offers to **install Ollama** (deep-link / Homebrew) and to **pull the configured roster from inside the app**, streaming `pull` progress onto the existing [`progress` seam](run-tracking.md).
The app never starts or stops the daemon (ruled 2026-08-27): its lifecycle is the user's, an unreachable daemon is a manual fix, and residency rides the per-request `keep_alive` the roster default sets.
(This is the *connectivity* surface — distinct from the presence warning that gates on unset config values, described above; a missing daemon or unpulled model is never a warning-area entry.)
Embedding the inference engine directly (mlx / llama.cpp linked as a Rust crate) was rejected for the same reason the runtime is Ollama at all — it would forfeit Ollama's default multi-model residency/eviction and its grammar-constrained `format` output, the two properties the roster design leans on.

## The model roster and per-task routing

The suite routes each kind of work to the model that does it best rather than running one model for everything.
The default roster:

- **`Qwen3.5-122B-A10B`** — the primary reasoner: deep research (with tool use), financial analysis, and writing.
  Run in its **thinking mode** for research, the write-ups, the hypothesis documents, the watchlist review, archetype confirmation, the analysis, the review, the thesis document and the action call, and in non-thinking mode for distilling write-ups and for transcribing a document's stated values into its typed appendix or decisions.
- **`Qwen3.5-35B-A3B`** — an **optional** fast tier for the cheap, high-throughput steps (distilling write-ups, routine routing).
  Demoted from the default resident set — see *One brain, two modes* below.

**One brain, two modes — not two brains.**
The default resident set is **the 122B reasoner alone**, and the 122B fills *every* reasoning role — research, distillation, analysis, review, interpretation and the action call — switching by **mode** (thinking for the research and the documents, non-thinking for distilling write-ups and for the typed appendix) rather than by model.
*(Version discipline — Ollama bug #14645: the fix is verified on the pinned v0.32.5, so a `format`-carrying `think: false` call is permitted on that version; the typed appendix is that call, and any Ollama bump re-locks it until the schema-integrity check passes on the new version.
See [local-model-operations.md](local-model-operations.md).)*
This is the deliberate default for two reasons.
First, it sidesteps the co-residency question entirely: one 122B at the target quantization fits 128 GB with comfortable headroom, where two large models never co-fit.
Second — because a single holding runs its research and synthesis conversations (thinking mode), where the analysis prompt is over budget the distillation of its write-ups (non-thinking mode — the merged write-ups, or each write-up first; see [web-research.md](web-research.md)), then its analysis, review, thesis document and action call (thinking mode) and the typed appendix (non-thinking mode) on the same model — a single resident model pays **no model-swap cost** switching modes within a holding, whereas a second large model not co-resident would.
The fast 35B tier is therefore an **option, not a requirement**: it is reintroduced only if (a) distillation wall-clock proves a measured bottleneck on the target hardware *and* (b) a 122B + 35B set **co-resides cleanly on an on-device benchmark** — otherwise the swap cost it adds would exceed the throughput it saves.
Either way the roster never runs more than one large reasoner; the only open question is whether a *small* second model earns its residency.

The roster is **user-configurable** within the local provider (model ids and the Ollama endpoint live in settings — see [configuration.md](configuration.md)); the defaults above are the recommended fit for the target hardware, not a hard-coded set.

Operational best-practices for running the default reasoner — context limits, sampling settings, thinking-mode mechanics, the `num_ctx` / structured-output gotchas, and the M5 serving pre-flight — live in [local-model-operations.md](local-model-operations.md).

## The local-model adapter seam

The cloud agents select a model from a closed enum with hard-coded provider endpoints.
The local suite uses a separate, **flexible** adapter instead: a call is parameterized by `{ endpoint, model_id, messages, tools, format_schema, options }`, so a roster of models behind one endpoint is addressed by id without enumerating each as a compile-time variant.
This keeps the cloud `AgentModel` enum untouched and lets the roster change through configuration.

The suite makes **no embedding call**: neither local job writes or reads vector memory ([storage.md §Local Vector Memory](storage.md#local-vector-memory)), so the report's `Embedder` trait and its response validator are untouched by the local suite.
Token and reasoning streaming ride the existing `progress` seam ([run-tracking.md](run-tracking.md)), so a local job streams per-step progress, per-request rows, and model output into the run tracker exactly as a report run does.
Every local chat call also emits a call-boundary event pair on that seam for the diagnostic thought log, which the tracker does not render ([run-tracking.md §Thought-log capture](run-tracking.md#thought-log-capture-diagnostic)).
Each resolved physical chat attempt produces one observation before downstream validation, including transport failures, broken streams, length stops, and retries.
The adapter outcome says whether the adapter returned a response, never whether that response passed the caller's semantic checks.
The observation retains the exact request stage (holding, topic, gathering or synthesis leg, turn, and expanded-attempt suffix where applicable), model, thinking flag, streaming mode, tools/format flags, and declared reservations.
`prompt_chars` is the existing serialized messages-plus-tools character count before daemon processing, including roles and assistant tool calls but excluding transport controls and the output grammar; it measures the issued packet, not evidence before app compression.
`elapsed_ms` is monotonic app wall time for that physical attempt, including connection, daemon processing and response reading, but excluding retry waits and downstream validation.
Thinking is counted in decoded Unicode characters with an explicit `complete`, `partial`, or `unavailable` state: a complete reply without thinking records zero, an interrupted stream retains the characters received, and a failure before an observable reply records unavailable.
The six raw API fields are stored under `api`: `prompt_eval_count`, `eval_count`, `total_duration`, `load_duration`, `prompt_eval_duration`, and `eval_duration`; durations retain their nanosecond units and omitted fields remain absent values, never zero or estimates.
On the pinned thinking-plus-format runtime, `eval_count` and `eval_duration` describe the second task, not thinking-plus-content totals; the terminal prompt count need not describe the original packet, while `total_duration` can include both phases ([local-model-operations.md §M5 pre-flight checklist](local-model-operations.md#m5-pre-flight-checklist)).
A format request with thinking on or left to the runtime default is therefore treated conservatively as phase-limited: a length stop remains a failure but its counters do not attribute it to context exhaustion or the whole-call output reservation.
The reported prompt fill can still identify pressure in the measured runtime phase; comparing its count with the original packet cannot establish front-truncation on that path.
Single-phase calls retain the existing likely-front-truncation and length-stop diagnostics, and retry eligibility, reservations, and limits are unchanged.
The complete observation list survives in the finished run's data health; checkpoint/resume retains the holding-row ownership described in [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture).
An abrupt process termination can leave only a call-fence header; telemetry does not add per-attempt crash persistence.
The transport deadline on every chat call is derived from the request's own reservations: `num_ctx` over a prompt-evaluation floor, plus `num_predict` over a decode floor on the non-streaming path, so transport cannot cut a chain that stays inside its reservation while the daemon holds the drafted floors and its pre-generation overhead fits in the slack the unused reservation leaves.
That slack is the unused part of the budget — the context the prompt does not fill at the prompt-evaluation floor, plus, on the non-streaming path, the output the chain does not generate at the decode floor — and it is what absorbs runner scheduling and a cold model load ahead of the first byte.
It is never zero on the non-streaming path, since generation shares the context with the prompt, but a streaming call keeps only what its prompt leaves.
No derived deadline goes under the ten-minute backstop, which a request declaring no reservation rides exactly.
The streaming path takes the prompt-evaluation term alone, since its tokens arrive as they generate, and that same value bounds each body read as an idle limit.
The floors are drafted from the pinned serving path's measured throughput and move only with it ([local-model-operations.md §M5 pre-flight checklist](local-model-operations.md#m5-pre-flight-checklist)).
The transport deadline composes with a **bounded retry-once** above it: a failed chat call re-attempts exactly once when the failure classifies as transient — a transport-level connection failure, a daemon error status, an empty completion body, a schema-parse failure of the returned content, or a broken stream — after a short drafted pause, each attempt deriving its own deadline.
A typed appendix whose value falls outside its declared domain classifies transient too, as its own class, so the data-health read tells an off-domain value from malformed content (the domain is canonical at [portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)).
The once is per issued call, and no call path nests a second retry layer.
A required appendix field returned null is no retry class: the orchestrator reissues the thesis-document message once as a new call with its own appendix message, each keeping this once ([trade-opportunities-workflow.md §Step 5h](trade-opportunities-workflow.md#step-5h-deterministic-risk-tier-gate-validation--checkpoint)).
The classification is a whitelist: a deadline trip, a length stop, a cancelled run, and any unclassified failure never enter the general retry.
A phase-limited length stop remains unattributed, not a reason to repeat the same request.
Distillation has one narrower output-sizing exception outside that general retry: a call that reports exactly the normal 12,288-token reservation gets one re-attempt at a 32,768-token ceiling on the reasoner's 128 K context.
A length stop below the normal reservation is context-bound or unattributable and fails without that re-attempt; any length stop on the expanded call also fails hard.
The expanded attempt is final for that stage: the outer schema/transport gate cannot layer another request after it.
The action call's blank-rationale guard keeps its fail-hard posture outside the retry.
A second failure fails hard as before, annotated with the first attempt's class, so the failure detail stays attributable — the seam is job-agnostic; what a hard failure fails (the report run, or one Portfolio holding isolated) is each job's §Failure posture.
Every fired retry emits its own tracker row and lands on the run's data-health read as a summary line plus structured events — the big confirmation run's transient-rate measurement.
Each event names the stage that re-attempted, and a research-loop event names the holding step, the topic, and the leg — a gathering turn or the synthesis call — so a retry is attributable to the topic it fired on, not only to the holding.
The tracker row's detail and each event's cause carry the class name followed by the failed attempt's full error chain, so a parse failure names its innermost message and a head-and-tail snippet of the body.
A resumed run's read covers the calls behind the finished run's verdicts — every restored row's and every call of the resumed process — and omits only the superseded calls of holdings the resumed process re-analyzed ([portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)).

Every distillation call is also **sized at issue**: the adapter measures the rendered prompt in chars against its model's input budget before any request exists.
The measure is the whole rendered prompt — the instruction scaffolding and the write-ups it distills together.
The budget is the same threshold-and-chars-per-token budget the consolidation routing uses ([configuration.md §Research Context Management](configuration.md#research-context-management)).
A prompt within the fast tier's budget issues there.
One over it but within the reasoner's issues on the resident reasoner at its interpretation context — a model choice, never a `num_ctx` change, so no runner reloads and the one-`num_ctx`-per-model rule stands.
The fast tier co-resides by the roster's own precondition ([§The model roster and per-task routing](#the-model-roster-and-per-task-routing)), so a route-up costs no swap.
A prompt over the widest budget is refused before issue as an unclassified failure — never retried, since the outcome is deterministic — and fails hard under the job's hard model-call posture (the report run, or one Portfolio holding isolated — each job's §Failure posture).
The guard covers every distillation call — the merge distillation and each per-write-up distillation — closing the daemon's silent front-truncation off from distillation as far as a chars-per-token estimate can close it.
The budget is that estimate — the chars-per-token constant is rough and the guard counts characters — so token-dense input can fit the threshold and still overflow the context.
The data-health likely-front-truncation read therefore stays the runtime witness for every stage, distillation included.
A merge distillation that outgrows the widest issuable budget once rendered — the reasoner's on a distinct roster — takes the next smaller shape before it reaches the guard: each write-up distilled first, then the merge of those outputs ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)).
One over the fast tier's budget but within the reasoner's still issues and routes up, so the smaller shape is never taken for a prompt the reasoner could serve.
A single write-up is bounded by the synthesis reservation that wrote it, so the guard binds only where no smaller shape remains.
On the default roster (a blank fast tier) the two rungs are one budget, and only the refusal is live.

## Schema-constrained output

The suite's typed outputs — the thesis document's appendix of conviction and the three expected prices, and the action call's rung and rationale — are **schema-validated JSON objects**, produced with grammar-constrained decoding (Ollama's native `format` schema).
The research documents — the write-ups, the analysis, the review and the thesis document — are prose and carry no grammar.
The grammar is the generation constraint, not the application's only validator: a served call can still return an empty or otherwise non-decodable body, so the app parses every typed response and classifies a structural violation at the stage boundary as the bounded-retry `SchemaParse` class.
The appendix keeps only the type check — each expected price finite and strictly positive, a value a share price can take — and the action call's rung must be one of its five values with a nonblank rationale.
For a financial pipeline whose persisted records and outcome scoring depend on well-formed prices and actions, deterministic structure is load-bearing wherever structure is asked for — a free-form-JSON parse-and-pray path is not acceptable here.

## Prompt posture

Grammar constrains an output's *structure*; the prompt supplies the model's *inputs* — the evidence, the engine's reads, the deterministic facts, and any degradation in them — and states the task's constraints.
Governed prompt content is traceable to a canonical project contract: field meanings, output requirements, source-provenance policy, continuity rules, and explicitly ruled decision frameworks such as Portfolio action precedence.
Ungoverned prompt content does not belong: app or downstream-consumer architecture, meta-reasoning instructions, author-added financial preferences, or weighting and conclusion nudges without a canonical contract.
Within the governed constraints, the model draws the inference from the supplied facts; the prompt does not add an unruled conclusion for it to reproduce.
The research pass's gathering degradation is a persisted gap only; the write-up states what the evidence leaves unanswered ([web-research.md §The research loop](web-research.md#the-research-loop-and-context-management)).
No Portfolio prompt names an app concept: each is one message in two parts, the data with its glosses and then the task, a typed call closing on a placeholder-only return shape, and none carries pipeline-architecture narration, meta-reasoning, or an ungoverned how-to-weigh nudge.
This is the same "informs, never dictates" stance the suite takes with values (§Context-memory discipline) and with source quality ([web-research.md §Source quality and evidence weighting](web-research.md#source-quality-and-evidence-weighting)): the app surfaces the fact and the model judges.

## Context-memory discipline

The suite's stages chain through **bounded documents and typed objects**, never raw transcripts.
Each stage emits a bounded document — a write-up, the analysis, the review, the thesis document — or a validated typed object; the next stage receives only that plus the specific evidence it needs — not the prior stage's full conversation.
Four rules enforce this:

- **Deterministic packet assembly.**
  Per-item evidence packets (e.g. a holding's dossier) are assembled by the Rust application layer, the same way the report pipeline builds its condensed research packet deterministically rather than letting the agent gather unbounded context (see [report-workflow.md](report-workflow.md)).
- **Retrieve, don't dump.**
  Market Signal Report context enters a stage through a **deterministic last-X-report load** — the latest report's relevant sections plus recent report summaries, reusing the report pipeline's own recent-reports loader — never by vector-searching the report's memory.
  Continuity context from the job's *own* prior runs enters one way: an item's own prior documents — for Portfolio, the holding's prior position, prior analysis and prior thesis document; for Trade Opportunities, the lifecycle's prior thesis document, analysis and appendix values — load **deterministically by identity**, never by replaying whole runs and never by vector retrieval.
- **Forward only what's needed.**
  A holding's research reaches interpretation as its analysis — one document consolidating the topics' write-ups, which are distilled first (by the reasoner in non-thinking mode, or the fast 35B tier if resident) only where the analysis prompt is over budget — so interpretation reasons over the research's account of the evidence, never over the research conversations.
  (The distill call runs non-thinking with an explicit `think: false` on the wire and no grammar; the version-discipline rule in [local-model-operations.md](local-model-operations.md) governs the one `format`-carrying non-thinking call, the typed appendix, re-locking it on any unverified version bump.)
- **Compute, don't guess — and compute the baseline.**
  Quantitative finance — metrics, sub-scores, risk tiers, valuation multiples, volatility, concentration, and scenario price targets — is computed by the Rust application layer (a deterministic financial-analysis engine), never adopted from model output: every **baseline-arm** value is computed, and the model's own arm is authored beside that baseline, never in place of it.
  **Both suite jobs are two-arm**: the engine's values are the **baseline arm**, and the model additionally authors its **own** clearly-typed arm of the judgment fields — in **Portfolio Analysis** the conviction and the expected share price at three months, twelve months and three years, argued in its thesis document, and the action with its own one-line rationale from a separate call; in **Trade Opportunities** the risk tier and horizon that place the matrix card, the conviction, the expected price at the same three horizons, the detection mode, the leading metric with its class and on a carried name the status, argued in its thesis document and transcribed into its typed appendix, while the engine's rule-derived pair stays the baseline and the gate's scale ([trade-opportunities.md §The opportunity space](trade-opportunities.md#the-opportunity-space)) — checked on type alone, never against the engine's values ([portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)), with each job's accuracy checks scoring both arms against realized prices.
  What the checks score is each arm's price view: the model's expected prices and the engine's base values by percentage error against the close at each of the three horizons, into per-holding or per-lifecycle accuracy scores ([portfolio-analysis.md §Outcome learning](portfolio-analysis.md#outcome-learning-calibration), [trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)); conviction, tier and horizon are recorded unscored, and nothing is derived beyond the scores.
  Two rules hold on both sides: engine-owned values are **app-stamped directly, never echoed through the model** (so no exact-equality echo check is needed on either job — the round-trip that made one necessary is gone), and the model arm's judgment values never alter or bind the engine baseline, which is what keeps a missing input a gap rather than a fabricated level on the arm the machinery trusts.
  Each job single-homes its own boundary statement, with its intentional downstream consumers: [portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict) and [trade-opportunities.md §The opportunity](trade-opportunities.md#the-opportunity).
  The line is not engine-versus-model but **fact-versus-judgment**: facts and their arithmetic stay single-valued (a second version of a price or an Altman Z is fabrication, not judgment), as does each job's outcome machinery — whoever keeps score cannot also be a player — while judgments about the future are carried twice.
  The jobs differ in what a second arm is permitted to *do*: Portfolio's model arm annotates a fixed universe it cannot add to, while Trade Opportunities' can **admit** a name its engine arm's entry gate would have refused, recorded as such and measured ([trade-opportunities.md §The opportunity](trade-opportunities.md#the-opportunity)).

Why it's load-bearing: bounded, structured context is what keeps long multi-call jobs inside the memory budget *and* what curbs run-to-run drift — the analysis stage cannot be swayed by incidental phrasing in an upstream transcript it never receives.

## Run history and continuity

Each local job persists its results as a **run**, retaining the most recent N runs per feature (the cap is per-feature, parallel to the report-retention rule in [storage.md](storage.md)).
Two uses follow:

- **Continuity input.**
  The prior run's per-item verdict feeds the next run — for Portfolio, the holding's prior position, its prior analysis and its prior thesis document; for Trade Opportunities, the lifecycle's prior thesis document, analysis and appendix values.
  A change in action, conviction, expected price, or opportunity status must be justified by what materially changed — the same conviction-with-continuity doctrine the report thesis follows ([thesis-continuity.md](thesis-continuity.md)), applied per holding and per opportunity.
  Output is firm and directed; it does not swing between runs absent hard supporting data.
- **No semantic recall.**
  Neither job writes or reads vector memory: Portfolio Analysis's continuity is the deterministic load above, and Trade Opportunities' the deterministic load of the lifecycle's own documents by lifecycle id, a fresh re-entry from the archive loading nothing ([portfolio-analysis.md §Continuity and isolation](portfolio-analysis.md#continuity-and-isolation), [trade-opportunities.md §Continuity and isolation](trade-opportunities.md#continuity-and-isolation)).

**Each job's memory is its own.**
Vector memory holds the Market Signal Report's continuity learnings alone; the local jobs hold theirs in their own documents, and the report remains available to them as a **read-only shared input** that enters deterministically (see [§Context-memory discipline](#context-memory-discipline)) — never by searching the report's vector partition ([storage.md §Local Vector Memory](storage.md#local-vector-memory)).

## Web access

When a local stage needs the open web, it requests a tool call; the Rust orchestrator executes the search and fetch and returns the result — the agent never performs network I/O itself, holding the same pure-stage boundary as the report pipeline.
The web tool (SearXNG-only, plus readability extraction) is documented in [web-research.md](web-research.md).

## Failure posture

The research half of each local job is **fail-soft**: a flaky web search degrades the evidence for an item rather than failing the run.
The local-job execution gate — the daemon + roster **configured**, **a connected Schwab account** (required by both jobs, since holdings and the options-activity signal come from Schwab), the shared **FMP / FRED credentials present** ([portfolio-workflow.md §Step 1](portfolio-workflow.md#step-1-job-start-and-gate)), and the daemon actually **reachable** for any run that calls the model — is the precondition that blocks a run, and it is independent of the cloud-report gate.
A *configuration-blocked* local job surfaces in the Persistent Warning Area under its own presence-based categories (local models not configured — blocking the jobs that require the missing roster id — and Schwab connection, distinct from the report's) or, for the FMP / FRED keys, the shared **missing provider credentials** category.
A live connectivity failure caught at the run-gate blocks the attempt inline rather than as a persistent warning — except the engine-only paths that make no model call, ATO's **Quick Audit** and Portfolio's **Quick check**, which skip the daemon-connectivity check and run even with the daemon configured-but-down ([trade-opportunities.md §Failure posture](trade-opportunities.md#failure-posture), [portfolio-analysis.md §The quick check](portfolio-analysis.md#the-quick-check-engine-only)).
A local job that *fails mid-run* surfaces under the shared **Failed jobs** category, like a failed report ([interface.md §Connection status](interface.md#connection-status-local-suite)).
**A single global run slot serializes all jobs** — the report and both local jobs are mutually exclusive, so only one runs at a time, matching the latest-run-only run tracker.
Cancellation is cooperative through the shared `progress` seam, identical to a report run.
