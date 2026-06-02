# Synthesis

*One-page own-words summary of what is being built. Written by /metis-reconcile.*

## What it is

Market Signal is a **local-first desktop app** (Tauri + Vue, with SQLite and LanceDB) that runs a **scheduled weekly market-analysis job** and produces an evolving, professional-grade market report. It is explicitly *not* a trading bot and gives no buy/sell instructions; it is a thesis-generation and market-analysis system organized around weekly cadence to favor structural signal over daily noise. Everything runs on the user's machine except external API/model calls.

## The core loop

A single recurring job — the **Weekly Market Report** — runs Sundays at 9:00 AM local time (and can be triggered manually with the same validation and workflow). The job is a fixed **17-step pipeline** (weekly-report-workflow.md). In broad strokes: validate config and gates → load recent report context and audit prior reports → retrieve vector memory → check the research inbox → gather baseline market data → gather and filter news (a ~500-headline funnel narrowed to a handful of topics) → route research and execute a bounded research plan → build a condensed research packet → run three analyst agents → main-agent synthesis → save Markdown report + metadata + memory → generate HTML and update the UI.

## Agents

The pipeline is **agent-driven but not agent-controlled**: analyst stages are mandatory, and the application layer (not the agents) performs external API calls and deterministic retrieval. A **Main Agent** ("Head Market Analyst") plans research needs, consumes curated data, builds the condensed research packet, critiques analyst output independently (no recursive back-and-forth), maintains the evolving long-term thesis, and writes the final report in one unified voice. Three **analyst agents** — Bull, Bear, Balanced — each receive the same packet and produce structured perspective analysis; they are professional analytical lenses, not forced disagreement, and may all agree. The user configures the model for these four agents (OpenAI GPT-5 / GPT-5 mini, or Anthropic Claude Opus / Sonnet / Haiku). Separately, **fixed non-configurable models** handle internal stages: GPT-5 mini for headline filtering and data extraction, Claude Sonnet for research routing, and text-embedding-3-large for embeddings. A library of **16 reusable analyst skills** (structured prompts with output schemas) is declared for MVP.

## Memory & continuity

The system treats analysis as a continuous long-term thesis, not disconnected snapshots. Each report flows from prior ones: it follows up unresolved risks, audits whether past calls were directionally correct, and evolves the thesis gradually — staying stable through noise but pivoting decisively on major structural change. **LanceDB** holds long-term semantic memory (report summaries, durable learnings, thesis evolution, analogs, past mistakes); the app retrieves relevant fragments to guide the main agent. The audit step reviews roughly the previous 2–6 reports.

## Reports, storage, export

Reports are authored and stored canonically in **Markdown** — agents only ever read Markdown, never HTML. An **HTML** version is generated (via markdown-it) purely for in-app rendering, charts, and PDF export. Reports follow a standard section layout (Header Summary, Market Regime, Index Picture, Key Market Drivers, Market Signal Thesis, Retrospective Audit, Investment Strategy, Forward Outlook, Watchlist, Sources). **SQLite** stores records, metadata, HTML, job history, and warning states, including a structured report-summary JSON schema and a single `market_regime` label from a fixed 6-value vocabulary. Only the most recent **30 reports** are retained (cascade-deleting Markdown, HTML, metadata, and vector summary refs), though durable learnings persist beyond report deletion. Users can **export** any report as Markdown or PDF without re-running the workflow.

## Data, config, and runtime posture

External data flows from **FMP** (primary financial source, via REST from Rust), **FRED**, **BLS**, **Tavily** (primary news/research), and **GDELT** (geopolitical). The user supplies OpenAI and Anthropic API tokens plus provider credentials (FMP and Tavily, both required); FRED/BLS/GDELT have public APIs. The app ships in an **incomplete configuration state**: jobs are enabled by default but blocked until all four agent models and required credentials are set. Runtime is honest about its limits — jobs run only while the app is running (system tray counts), never during sleep, never concurrently (second runs are *skipped*), and missed runs are surfaced but not replayed. Job outcomes are classified as Successful / Failed / Missed / Skipped, and a de-duplicating **Persistent Warning Area** surfaces config gaps and job problems.

## Where the corpus is thinner

The docs are internally cross-referenced and mostly coherent, but a few seams remain: the division of research-planning ownership between the Main Agent and the fixed routing model, the exact token/credential validation gate (especially that fixed internal models force *both* LLM providers regardless of agent selection), and how the declared 16 analyst skills actually plug into the 17-step pipeline. These are tracked in CONTRADICTIONS.md and QUESTIONS.md.
