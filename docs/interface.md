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
│   ├── Per-holding verdicts (standing thesis + intrinsic verdict + portfolio action + thesis monitor)
│   └── Portfolio roll-up & construction
│
├── Trade Opportunities (local analysis suite)
│   └── Risk × horizon matrix (high/med/low × short/mid/long)
│
├── Persistent Warning Area
│   ├── Missing agent configuration
│   ├── Missing API tokens
│   ├── Missing provider credentials
│   └── Failed jobs
│
└── Settings
    ├── Agent model configuration
    ├── API token configuration
    ├── External data provider credentials
    ├── Report generation
    ├── Local analysis models (daemon endpoint + roster)
    ├── Web research (SearXNG endpoint)
    ├── Investor profile (risk tolerance, horizon, objective, tax, cash)
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

The local analysis suite adds its own warning categories, both following the same one-warning-per-category de-duplication and both **blocking** the local jobs: **local models unavailable** (daemon unreachable or roster missing) and **Schwab connection** (not connected or re-authentication required) — a connected Schwab account is a hard precondition for both jobs, since holdings and the options-activity signal come from it, so manual-import holdings do not clear this gate. Detailed per-state UI for the local pages (stale holdings, expired OAuth, partial results, not-rated assets, empty matrix cells) follows the project's frontend-craft state requirements. The Portfolio page must present each holding's **intrinsic verdict and final portfolio action as distinct but linked**, with the action's portfolio-context rationale visible (see [portfolio-analysis.md §The holding verdict](portfolio-analysis.md#the-holding-verdict)), so a deliberate pairing like *A-grade / trim* reads as intentional rather than contradictory. Each holding card is **anchored by the thesis ledger's current standing thesis** — the *why we hold this view*, rendered from the ledger ([portfolio-analysis.md §The position thesis ledger](portfolio-analysis.md#the-position-thesis-ledger)), not a separately authored summary — so the grade, action, monitor, and what-changed line read as evidence supporting a stated thesis (the held-position analog of each Trade Opportunity leading with its directional thesis).

Operational triggers for each category live in their canonical homes:
- Missing agent configuration and missing API tokens — see [configuration.md](configuration.md).
- Missing provider credentials — see [configuration.md §External Data Provider Credentials](configuration.md#external-data-provider-credentials).
- Failed jobs — see [scheduling.md §Offline Behavior](scheduling.md#offline-behavior) and [scheduling.md §Error Handling](scheduling.md#error-handling).
