# Local Analysis Models

The **local analysis suite** — Portfolio Analysis ([portfolio-analysis.md](portfolio-analysis.md)) and Trade Opportunities ([trade-opportunities.md](trade-opportunities.md)) — runs entirely on local open-weight models on the user's machine. This is a deliberate boundary: the Market Signal Report uses the user-configurable cloud agents (OpenAI/Anthropic — see [agents.md](agents.md)), while the analysis suite is **local-only, keyless, and cost-free at the model layer**. The two halves share the app's deterministic spine but not their model providers.

This document covers the substrate both local features build on: the serving runtime, the model roster and how work is routed across it, the adapter seam, schema-constrained output, the context-memory discipline that governs multi-call stages, and the run-history continuity pattern. The per-feature pipelines live in their own documents.

## Serving runtime

Local models are served by a single **Ollama** daemon running its **MLX backend** (Apple-Silicon GPU path), exposing an OpenAI-compatible HTTP API the Rust backend calls with `reqwest::blocking` behind the same `spawn_blocking` seam used for the cloud agents (see [agents.md](agents.md)). Ollama is the chosen runtime because it is the one that, by default, keeps **several models resident simultaneously** and queues/evicts them by memory pressure — the posture the "route each role to the best model" design needs — while the MLX backend gives the GPU throughput.

**Rationale (Ollama over alternatives):**
- multi-model residency and on-demand swap are the default, not an opt-in;
- one OpenAI-compatible surface for chat *and* embeddings, reachable from the existing reqwest adapter;
- MLX backend closes the historical Apple-Silicon speed gap.

**Schema-constrained output uses Ollama's native `/api/chat` `format` parameter, not the `/v1/` OpenAI-compatible path.** The `/v1/` layer advertises only JSON mode; reliable JSON-Schema conformance comes from passing the schema to the native endpoint. This is the one place the local adapter diverges from a plain OpenAI client (see [§Schema-constrained output](#schema-constrained-output)).

**Lifecycle.** The app supervises the daemon: it health-checks the endpoint at startup, and a local job will not start unless the daemon is reachable and the configured roster is present. A missing or unreachable daemon surfaces as a blocked local job through the warning area, parallel to (but independent of) the cloud-report execution gate — a machine with no cloud keys can still run the local suite, and vice versa (see [configuration.md](configuration.md)).

## The model roster and per-task routing

The suite routes each kind of work to the model that does it best rather than running one model for everything. The default roster:

- **`Qwen3.5-122B-A10B`** — the primary reasoner: deep research (with tool use), financial analysis, and synthesis/writing. Run in its **thinking mode** for multi-step financial reasoning and in non-thinking mode for firm, directed prose.
- **`Qwen3.5-35B-A3B`** — the fast tier: distilling raw research into compact findings, routine routing decisions, and drafts. Fast enough to keep multi-call stages responsive.
- **`Qwen3-Embedding-4B`** — embeddings for the suite's vector memory (see [§Run history and continuity](#run-history-and-continuity)).

**One big brain, two modes — not two big brains.** The roster deliberately pairs *one* large reasoner with a small fast model. Two 120B-class models do not co-fit in 128 GB of unified memory alongside working context. The intended posture is that the 122B, the 35B, and the embedding model stay resident together — but that depends on quantization, context length, and KV-cache size, so it is **gated on an on-device benchmark, not assumed**; if the measured footprint leaves too little headroom, the suite evicts the fast model between stages or hot-swaps, managed by the daemon. Where the report pipeline distinguishes "research" from "analysis" by model, the local suite distinguishes them mostly by **mode** (thinking vs non-thinking) on the same reasoner, using the fast model for the cheap, high-throughput steps in between.

The roster is **user-configurable** within the local provider (model ids and the Ollama endpoint live in settings — see [configuration.md](configuration.md)); the defaults above are the recommended fit for the target hardware, not a hard-coded set.

## The local-model adapter seam

The cloud agents select a model from a closed enum with hard-coded provider endpoints. The local suite uses a separate, **flexible** adapter instead: a call is parameterized by `{ endpoint, model_id, messages, tools, format_schema, options }`, so a roster of models behind one endpoint is addressed by id without enumerating each as a compile-time variant. This keeps the cloud `AgentModel` enum untouched and lets the roster change through configuration.

The local embedder implements the **same `Embedder` trait** the report pipeline already defines, so vector-memory storage and retrieval are reused unchanged (only the vector space differs — see below). Token and reasoning streaming ride the existing `progress` seam ([run-tracking.md](run-tracking.md)), so a local job streams per-step progress, per-request rows, and model output into the run tracker exactly as a report run does.

## Schema-constrained output

Every structured hand-off between stages is a **schema-validated JSON object**, produced with grammar-constrained decoding (Ollama's native `format` schema). The model picks values; it cannot emit invalid structure. For a financial pipeline whose downstream stages and persisted records depend on well-formed grades, targets, and actions, deterministic structure is load-bearing — a free-form-JSON parse-and-pray path is not acceptable here.

## Context-memory discipline

The suite's stages chain through **distilled, schema-shaped hand-offs**, never raw transcripts. Each stage emits a compact validated object; the next stage receives only that object plus the specific evidence it needs — not the prior model's full output. Four rules enforce this:

- **Deterministic packet assembly.** Per-item evidence packets (e.g. a holding's dossier) are assembled by the Rust application layer, the same way the report pipeline builds its condensed research packet deterministically rather than letting the agent gather unbounded context (see [report-workflow.md](report-workflow.md)).
- **Retrieve, don't dump.** Market Signal Report context enters a stage through a **deterministic last-X-report load** — the latest report's relevant sections plus recent report summaries, reusing the report pipeline's own recent-reports loader — never by vector-searching the report's memory. Continuity context from the job's *own* prior runs enters through **vector retrieval of the relevant slice** of that job's partition, not by replaying whole runs.
- **Forward only what's needed.** A research stage's output is condensed (by the fast model) into a findings object before the interpretation stage sees it, so interpretation reasons over evidence, not over the research transcript.
- **Compute, don't guess.** Quantitative finance — metrics, sub-scores, risk tiers, valuation multiples, volatility, concentration, and scenario price targets — is computed by the Rust application layer (a deterministic financial-analysis engine), not produced by the model. The model *interprets* those computed values and explains them; it never invents a number. This keeps the suite faithful to the app's data-honesty stance: a missing input becomes a gap, never a fabricated level.

Why it's load-bearing: bounded, structured context is what keeps long multi-call jobs inside the memory budget *and* what curbs run-to-run drift — the analysis stage cannot be swayed by incidental phrasing in an upstream transcript it never receives.

## Run history and continuity

Each local job persists its results as a **run**, retaining the most recent N runs per feature (the cap is per-feature, parallel to the report-retention rule in [storage.md](storage.md)). Two uses follow:

- **Continuity input.** The prior run's per-item verdict feeds the next run. A change in a grade, action, target, or opportunity status must be justified by what materially changed — the same conviction-with-continuity doctrine the report thesis follows ([thesis-continuity.md](thesis-continuity.md)), applied per holding and per opportunity. Output is firm and directed; it does not swing between runs absent hard supporting data.
- **Semantic recall.** Run results are embedded into vector memory so a later run of *the same job* can retrieve the relevant prior analysis for a given item.

**Each job's learning memory is isolated to that job.** Vector memory holds each job's accumulated continuity learnings across three independent partitions — the Market Signal Report, Portfolio Analysis, and Trade Opportunities — and a job writes and reads **only its own**. A job's learnings are specific to its work (holding-grading calibration is not opportunity-discovery context, and neither is market-thesis memory), so cross-job recall would be noise. This isolation governs *learnings only*: the Market Signal Report remains available to the local jobs as a **read-only shared input**, but it enters deterministically (see [§Context-memory discipline](#context-memory-discipline)) — never by searching the report's vector partition. Isolation is enforced by an explicit **job namespace** on each memory row — orthogonal to the existing summary/learning entry kind — with every retrieval scoped to the calling job's namespace, because the two local jobs share an embedder and dimensionality and so are not separated by dimensionality alone (the report is additionally separated structurally: it embeds with OpenAI `text-embedding-3-large`, a different dimensionality from the local embedder). All three reuse the same `vector_memory` module and `Embedder` trait; they differ only in which partition they touch (see [storage.md](storage.md)).

## Web access

When a local stage needs the open web, it requests a tool call; the Rust orchestrator executes the search and fetch and returns the result — the agent never performs network I/O itself, holding the same pure-stage boundary as the report pipeline. The web tool (SearXNG-primary, with a Tavily fallback, plus readability extraction) is documented in [web-research.md](web-research.md).

## Failure posture

The research half of each local job is **fail-soft**: a flaky web search degrades the evidence for an item rather than failing the run. The local-job execution gate (daemon reachable, roster present, and **a connected Schwab account** — required by both jobs, since holdings and the options-activity signal come from Schwab) is the precondition that blocks a run, and it is independent of the cloud-report gate; a blocked or failed local job surfaces in the Persistent Warning Area under its own categories (local models unavailable, Schwab connection), distinct from the report's. **A single global run slot serializes all jobs** — the report and both local jobs are mutually exclusive, so only one runs at a time, matching the latest-run-only run tracker. Cancellation is cooperative through the shared `progress` seam, identical to a report run.
