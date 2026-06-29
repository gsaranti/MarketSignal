# Application Interface

## Main Layout

```text
Market Signal
├── Latest Report View
│   ├── Rendered HTML report
│   └── Export actions
│
├── Run Tracker (shown in place of the report while a job runs)
│   ├── Per-step progress
│   ├── Per-request pass / fail
│   ├── Streamed agent output
│   └── Cancel control
│
├── Recent Reports Sidebar
│   ├── Ordered descending
│   ├── Report timestamps
│   └── Market Signal reports
│
├── Research Documents
│   ├── Research Inbox
│   └── Research Archive
│
├── Portfolio (local analysis suite)
│   ├── Holdings (manual pull / CSV import)
│   ├── Per-holding verdicts (standing thesis + intrinsic verdict [grade + forward outlook] + portfolio action + thesis monitor)
│   └── Portfolio roll-up & construction
│
├── Trade Opportunities (local analysis suite)
│   ├── Risk × horizon matrix (high/med/low × short/mid/long; each card: thesis · price prediction · conviction · leading metric · since-flagged return · became-an-opportunity & last-deep-research dates)
│   └── Archived opportunities (departed picks, price-tracked; each: frozen thesis · since-flagged return vs sector/market · became-an-opportunity & departure dates — no forward prediction)
│
├── Persistent Warning Area
│   ├── Missing agent configuration
│   ├── Missing API tokens
│   ├── Missing provider credentials
│   ├── Failed jobs
│   ├── Local models not configured   (local suite)
│   └── Schwab connection              (local suite)
│
└── Settings
    ├── Agent model configuration
    ├── API token configuration
    ├── External data provider credentials
    ├── Report generation
    ├── Local analysis models (daemon endpoint + roster + connection status)
    ├── Web research (SearXNG endpoint + connection status)
    ├── Connected sources (optional paywalled-subscription logins + per-source health)
    ├── Investor profile (read-only preset — risk tolerance, horizon, objective, tax, cash)
    ├── Charles Schwab connection
    └── Trade Opportunities discovery breadth (candidate research budget)
```

The operational behavior of each panel is defined in the relevant concern files:
- Latest Report View / Recent Reports Sidebar — see [report-structure.md](report-structure.md) and [storage.md](storage.md).
- Run Tracker — see [run-tracking.md](run-tracking.md).
- Export actions — see [export.md](export.md).
- Research Documents (Inbox / Archive) — see [research-documents.md](research-documents.md).
- Persistent Warning Area triggers — see [scheduling.md](scheduling.md) and [configuration.md](configuration.md). De-duplication behavior is described below.
- Settings — see [configuration.md](configuration.md) and [scheduling.md](scheduling.md).
- Portfolio — see [portfolio-analysis.md](portfolio-analysis.md) and [schwab-integration.md](schwab-integration.md).
- Trade Opportunities — see [trade-opportunities.md](trade-opportunities.md).
- Local analysis suite substrate and its settings — see [local-models.md](local-models.md), [web-research.md](web-research.md), and [configuration.md](configuration.md). Both local jobs stream into the same Run Tracker as a report run ([run-tracking.md](run-tracking.md)).

## Persistent Warning Area

The Persistent Warning Area surfaces:
- Missing agent configuration
- Missing API tokens
- Missing provider credentials
- Failed jobs

Each warning category may have at most one unresolved warning at a time. If a warning already exists in a category and has not been dismissed or resolved, additional events in that category do not create duplicate warnings.

Dismissing a warning permanently removes it. A subsequent event in the same category produces a fresh warning.

The local analysis suite adds its own warning categories, both following the same one-warning-per-category de-duplication, both **blocking** the local jobs, and both **presence-based** (they fire on missing *configuration*, not on a live connectivity probe): **local models not configured** (the Ollama endpoint or a roster id is unset) and **Schwab connection** (not connected or the refresh token has lapsed) — a connected Schwab account is a hard precondition for both jobs, since holdings and the options-activity signal come from it, so manual-import holdings do not clear this gate. Live **connectivity** failures are *not* persistent warnings: a local-model failure (daemon unreachable, a model not pulled) surfaces at the run-gate as an inline block on the run attempt, while a Schwab *API* outage surfaces when holdings are fetched (Step 2), not at the run-gate ([§Connection status](#connection-status-local-suite)). Detailed per-state UI for the local pages (stale holdings, expired OAuth, partial results, not-rated assets, empty matrix cells) follows the project's frontend-craft state requirements. The Portfolio page must present each holding's **intrinsic verdict and final portfolio action as distinct but linked**, with the action's portfolio-context rationale visible (see [portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)), so a deliberate pairing like *A-grade / trim* reads as intentional rather than contradictory. Within the intrinsic verdict the **backward grade and the forward outlook** (horizon reads + scenario targets) are presented as a **distinct, side-by-side pairing**, so a divergence — a weak grade with a constructive outlook, or a strong grade the market has already paid for — reads as intentional rather than as a glitch ([portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)). Each holding card is **anchored by the thesis ledger's current standing thesis** — the *why we hold this view*, rendered from the ledger ([portfolio-analysis.md §The position thesis ledger](portfolio-analysis.md#the-position-thesis-ledger)), not a separately authored summary — so the grade, action, monitor, and what-changed line read as evidence supporting a stated thesis (the held-position analog of each Trade Opportunity leading with its directional thesis). The **Trade Opportunities** matrix card likewise surfaces each idea's **price prediction as a first-class, user-facing element** — the engine's base-case scenario target with its bear / bull range over the fixed end-of-year (~12-month) window, its computation methodology accessible (an honest projection with a range, not a guarantee) — shown alongside the directional thesis, conviction, leading metric, catalyst, and (for a carried-forward idea) the since-flagged performance, so the **predicted target and the realized path sit together** ([trade-opportunities.md §The opportunity](trade-opportunities.md#the-opportunity), [§Storage and display](trade-opportunities.md#storage-and-display)). Detailed per-state rendering of both local pages follows the project's frontend-craft requirements.

Operational triggers for each category live in their canonical homes:
- Missing agent configuration and missing API tokens — see [configuration.md](configuration.md).
- Missing provider credentials — see [configuration.md §External Data Provider Credentials](configuration.md#external-data-provider-credentials).
- Failed jobs — see [scheduling.md §Offline Behavior](scheduling.md#offline-behavior) and [scheduling.md §Error Handling](scheduling.md#error-handling).

## Connection status (local suite)

Both local-suite backends the user self-hosts — the **Ollama daemon** and the **SearXNG instance** — expose a live connection indicator in their Settings section, built on the existing **`ConnectionTestRow`** pattern (a per-dependency "Test connection" control backed by the `test_connection` command, already used for the OpenAI/Anthropic/FMP/FRED/Tavily credentials). Each indicator reflects the **last connectivity check** — a manual *Test Connection* or the connectivity check run when a **job is launched** — Ollama's run-gate check, SearXNG's pre-run probe ([§Pre-run web-research notice](#pre-run-web-research-notice-local-suite)), *not* at app startup; with no startup probe, the indicator reads **untested** until the user tests or runs. The two are surfaced **asymmetrically**, mirroring their roles in the execution gate ([portfolio-workflow.md §Step 1](portfolio-workflow.md#step-1-job-start-and-gate)):

- **Ollama — gate-bearing, in two layers.** What *gates proactively* is **presence of the config values** (the Ollama endpoint + the roster ids — reasoner and embedder; the fast tier is optional): if any is unset the **Run buttons for Portfolio Analysis and Trade Opportunities are locked** and the persistent **local models not configured** warning shows — like the cloud model selectors / data-source tokens, cleared the instant the fields are filled. **Connectivity** (daemon actually reachable + rostered models actually pulled) is *not* probed at startup or on a timer — only at the **run-gate** (Step 1) and via a **manual *Test Connection***. A run-gate connectivity failure is an **inline block at the moment of clicking Run** (ephemeral, never a persistent warning), pointing to Settings → *Test Connection*. The Settings indicator then reports **endpoint reachability *and* per-roster-model presence** ("daemon up but the model isn't pulled" is a distinct state) and carries the matching **guided-setup** action — *Install Ollama* (deep-link / Homebrew) when unreachable, *Pull `<model>`* (with `pull` progress on the Run Tracker) when a model is missing (see [local-models.md §Serving runtime](local-models.md#serving-runtime)). Only the local-suite jobs are affected — the Market Signal Report runs on the cloud agents and a separate gate ([configuration.md §Local Analysis Suite Configuration](configuration.md#local-analysis-suite-configuration)). Because connectivity is never probed at startup, a config-set-but-daemon-down state shows no signal on re-open until the user clicks Run or tests — the deliberate, cloud-report-consistent trade for dropping the startup probe.
- **SearXNG — degradation, never blocking.** The status distinguishes **connected / running-but-misconfigured / unreachable** — a reachable instance that returns HTTP 403 (JSON output not enabled) is a *misconfiguration* with a different fix (re-run the shipped `docker compose up -d`) than a server that isn't running, so the row says which and deep-links to the docker-compose / OrbStack setup ([web-research.md §Search backend](web-research.md#search-backend-searxng)). A down SearXNG is rendered as an **informational** state with its consequence spelled out — *web research falls back to Tavily; Trade Opportunities discovery returns fewer candidates* — and is **never** a blocking Warning-Area category, because the suite's research half is fail-soft ([web-research.md §Tavily fallback](web-research.md#tavily-fallback)). The Tavily-fallback path is also visible at runtime as request rows in the Run Tracker.

These multi-state indicators reuse the **Connected Sources** health-state vocabulary and visual treatment (`connected` / `connected_but_thin` / `expired` / `unsupported` — [configuration.md §Connected Sources](configuration.md#connected-sources-subscriptions)) rather than introducing new status styling; per the design system, status uses existing tokens, not new colors. The two layers stay cleanly separated: the **presence** check (synchronous, always-known) drives the button-lock and the persistent *configuration* warning, while the **connectivity** check (`test_connection`, run at **job launch** or on manual test) drives the Settings indicator and the run-launch outcome — for Ollama the run-gate inline block, for SearXNG the pre-run degradation modal (off the execution gate) — presence gates proactively, connectivity is discovered at run time, mirroring the cloud report's *presence-not-connectivity* gate ([scheduling.md](scheduling.md)). No separate "connections dashboard" exists — Settings holds the live per-backend connectivity status and manual re-test, the Warning Area holds only the presence-driven *configuration* warnings, and the Run Tracker shows connectivity outcomes per request during a run.

## Pre-run web-research notice (local suite)

Because SearXNG is **off the execution gate** (§Connection status), a local job *starts* even with no web backend — but the fallback isn't free (it spends metered Tavily quota, or it degrades the analysis), and the consequences differ by job, so the app asks for **informed consent before spending the run**. When a run is launched the app runs a **live connectivity probe of the SearXNG instance** — an actual request to the endpoint, *not* merely a check that the endpoint value is set (which would be meaningless, since the endpoint has a default) — and if the instance **can't serve search** (unreachable, *or* reachable but misconfigured — e.g. an HTTP 403 with JSON output disabled, equally unusable), a **confirm modal** states what the user is about to run with and offers **Proceed / Cancel**; the probe result also updates the SearXNG Settings indicator. It is a consent step, **never a block** — SearXNG stays off the gate and the user can always proceed. The wording branches by job and by whether the Tavily credential is configured:

| Job | SearXNG down · Tavily configured | SearXNG down · no Tavily |
|---|---|---|
| **Portfolio Analysis** | Research falls back to **Tavily (metered)** instead of local search. | Web research is limited; the analysis leans on FMP / SEC / Stooq + the deterministic engine. |
| **Trade Opportunities** | **Model-led discovery can't run** — TO's discovery lane is SearXNG-only and does *not* fall back to Tavily ([web-research.md §Tavily fallback](web-research.md#tavily-fallback)), so candidates come only from the structured feeders + carried-forward watchlist; per-candidate **validation** does fall back to Tavily (metered). | **Model-led discovery can't run** *and* validation has no fallback → expect a **sparse matrix with insufficient-evidence abstentions**. Flagged **not recommended** — a stronger confirm (the run is proceed-able but the modal advises against it). |

Two points hold across all four. First, TO's *discovery* degradation is identical whether or not Tavily is configured — discovery never uses Tavily by design, so a configured Tavily rescues only TO's *validation* lane, not its candidate generation. Second, to avoid nagging on a persistently-down SearXNG, the modal offers **"don't ask again this session"** plus a Settings toggle to suppress it permanently, for the technical user who runs degraded knowingly. The modal reuses the project's confirm-dialog pattern under the frontend-craft dialog requirements (focus trap, Escape-to-cancel, focus restored on close).
