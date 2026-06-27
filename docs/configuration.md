# Configuration

## Settings Overview

The Settings section includes:
- model selection
- API token configuration
- external data provider credentials
- report generation controls
- local analysis suite configuration

The report generation controls are described in [scheduling.md](scheduling.md). This file covers model selection, API tokens, external data provider credentials, and the local analysis suite's own configuration.

## Agent Model Configuration

The user selects the models used for:
- Main Agent
- Bull Analyst
- Bear Analyst
- Balanced Analyst

OpenAI Models:
- GPT-5
- GPT-5 mini

Anthropic Models:
- Claude Opus
- Claude Sonnet
- Claude Haiku

By default, the application starts with no models selected for:
- Main Agent
- Bull Analyst
- Bear Analyst
- Balanced Analyst

The user must configure a model for all four agents before a report can be generated.

If any agent does not have a configured model:
- report generation is disabled
- the application displays a warning message on the homepage indicating which agents still require configuration

A report cannot be generated until all required agent models and provider credentials are configured.

A newly installed application therefore begins in an incomplete configuration state until the user finishes model and API setup.

For the non-configurable models used by fixed internal pipeline stages (headline filtering, research routing), see [agents.md §Fixed Internal Models](agents.md#fixed-internal-models).

## API Tokens

Both API tokens are always required, regardless of which models the user selects for the four agents:
- OpenAI API token
- Anthropic API token

Both are mandatory because the fixed internal pipeline stages always use both providers — OpenAI for headline filtering (GPT-5 mini) and for vector-memory embeddings (`text-embedding-3-large`), and Anthropic for research routing (Claude Sonnet). See [agents.md §Fixed Internal Models](agents.md#fixed-internal-models) and [storage.md §Embeddings](storage.md#embeddings). Because the only model providers are OpenAI and Anthropic, the user's agent model selection adds no token requirement beyond these two.

If either token is missing:
- saving the configuration is disabled
- the application displays a validation warning explaining which token is required

## External Data Provider Credentials

The application also requires configuration for external data providers that use authenticated APIs.

The Settings section includes credential configuration for:
- Financial Modeling Prep
- FRED
- Tavily

The **Financial Modeling Prep**, **FRED**, and **Tavily** credentials are all required to run a job:
- **Tavily** is the primary research and news-ingestion system, and news gathering is a mandatory workflow step (see [report-workflow.md §Step 7](report-workflow.md#step-7-gather-and-filter-news)).
- **Financial Modeling Prep** is the primary financial-data source and provides the equity-market portion of the baseline market-data scan — indices, volatility (VIX), gold, and sector performance (see [report-workflow.md §Step 3](report-workflow.md#step-3-gather-baseline-market-data)) — which is not optional, so a missing FMP credential blocks execution.
- **FRED** supplies the macro and commodity portion of the same baseline scan — the dollar index, oil, natural gas, and the 2Y/10Y Treasury yields — which is likewise not optional, so a missing FRED credential blocks execution. FRED requires a free API key on every request.

BLS, GDELT, and CFTC are also accessed through public APIs but need no Settings credential: **BLS** works keyless (an optional free key raises its rate limits), **GDELT** needs no key, and **CFTC** (the Commitments-of-Traders positioning source) is reached through the keyless Socrata public-reporting API.

If a required external provider credential (the Financial Modeling Prep, FRED, or Tavily credential) is missing:
- report generation is disabled
- the application displays a validation warning explaining which credential is missing

For the data providers themselves and what each is used for, see [data-sources.md](data-sources.md).

## Local Analysis Suite Configuration

The local analysis suite (Portfolio Analysis and Trade Opportunities) is configured separately from the report, and its settings gate the **local jobs only** — they are independent of the report's execution gate, so a machine set up for one need not be set up for the other.

### Local Models

The suite calls a local model daemon over an OpenAI-compatible HTTP endpoint. Settings hold:
- the **daemon endpoint** (the local Ollama URL)
- the **model roster** — the model ids for the reasoner, the fast tier, and the embedder (see [local-models.md](local-models.md) for the recommended defaults)

A local job is blocked unless the daemon is reachable and the configured roster is present.

### Web Research

The suite's web-research tool uses a local SearXNG instance, with the existing Tavily credential as a fallback (see [web-research.md](web-research.md)). Settings hold the **SearXNG endpoint**; no key is required for the local instance, and the Tavily fallback reuses the credential already configured above.

### Price Data

The suite's price and fundamentals load is spread across keyless providers (see [data-sources.md](data-sources.md)). **SEC EDGAR** and **Stooq** are keyless and need no configuration; live **quotes** come from the shared FMP key. Per-stock **option chains** come from the Schwab connection (below), and **CBOE**'s venue-level put/call backdrop is keyless — neither needs separate configuration.

### Charles Schwab Connection

Both local jobs source data from Charles Schwab via OAuth — Portfolio Analysis its holdings, and both jobs the options-activity signal from option chains (see [schwab-integration.md](schwab-integration.md)). Settings hold the developer **app key and secret** and manage the **connection state** (connect / re-authenticate); the app secret and the OAuth tokens are kept in the **macOS Keychain**, not the SQLite settings store, since they are bearer credentials to the brokerage account. **A connected Schwab account is required to run either local job** — it is part of the local-job execution gate, alongside the model daemon and roster. Because the OAuth refresh token expires every 7 days, both jobs are blocked with a re-authentication prompt when it lapses. Manual import can supplement holdings but does not satisfy the connection gate.

### Investor Profile

Both local jobs are personalized by an **investor profile**: risk tolerance, time horizon, objective, tax sensitivity, and cash posture. It shapes Portfolio Analysis's grading emphasis, action ladder, and cash/deployment stance, and Trade Opportunities' entry framing and conviction emphasis (see [portfolio-analysis.md](portfolio-analysis.md), [trade-opportunities.md](trade-opportunities.md)). The profile never changes *which* holdings grade well or *which* opportunities qualify — those are engine and research outputs — only how the prescription is framed for this investor.

**For now the profile is a fixed default preset, not user-configured** (a configurable profile is deferred). The default posture:

- **horizon — long-term.** The job favors durable multi-quarter / multi-year theses over short-term trades.
- **objective — maximize profit.** Total return is the goal; no income or capital-preservation mandate is imposed.
- **risk tolerance — medium-to-high.** Higher-risk cells and archetypes (disruptors, commodity cyclicals, smaller caps) are in scope, gated by the engine's forensic/risk discipline rather than by a conservative cap.
- **cash — always available.** Buying power is treated as **unconstrained**: the user may hold cash in accounts the app can't see, so *add aggressively* and full-size entries are **never** gated on observed Schwab cash. (Concentration and risk limits still apply; only the cash constraint is lifted.)
- **tax sensitivity — no precise modeling.** No tax-lot, holding-period, account-type, or marginal-rate calculation is applied to actions — the job never computes a tax harvest. It does treat the **possible tax benefit of realizing a loss** as one *generic, qualitative* counterweight once a position's forward prospects have already been judged poor, weighed beside the redeployment value of freed cash ([portfolio-analysis.md §Portfolio action](portfolio-analysis.md#portfolio-action)); it is framed as *possible* precisely because account type and rate are unmodeled, so the user judges their own specifics.

These defaults are the stated posture the suite runs against until a configurable profile exists.

### Trade Opportunities — Discovery Breadth

Trade Opportunities is a discovery funnel: it screens the whole universe, then spends expensive per-candidate validation (deep local-model research plus per-symbol data) only on a narrowed set ([trade-opportunities-workflow.md §Step 4](trade-opportunities-workflow.md#step-4-candidate-consolidation)). Because that per-candidate work runs on a local reasoner, one run cannot validate every surfaced name — so a **research budget** caps how many candidates get the full treatment per run. Settings expose this budget as a **discovery-breadth control**: a generous default, raised for a more exhaustive (longer) run or lowered for a faster one.

This is a **compute budget, not a quality cap**. At the point it applies nothing has been validated yet, so a name that doesn't fit the budget is **not rejected, only deferred** — and a genuinely worthy deferral (a real hypothesis + an identified leading metric) is **remembered**: written to the persisted **opportunity graph** as a watchlist node and re-checked every later run with its leading metric monitored, so a deferred name that quietly compounds is caught rather than lost to chance ([trade-opportunities.md §Discovery memory](trade-opportunities.md#discovery-memory-the-opportunity-graph)). (Only the *validated* opportunity matrix and this discovery graph carry forward through storage; a name not even worth watchlisting carries no state and is simply re-derivable by a later sweep.) The budget is always spent under the **diversity guardrails** (market-cap band / feeder / archetype / sector-theme), so a wider or narrower setting still can't collapse the funnel onto mega-cap momentum or one crowded theme. Crucially, the **final matrix has no output cap**: every candidate that clears the validation gates is listed, ranked by conviction ([trade-opportunities.md §The opportunity space](trade-opportunities.md#the-opportunity-space)) — the budget bounds only what gets *researched* per run, never how many *good, validated* ideas are shown.

Two **discovery-memory** settings bound that watchlist so it can't grow without limit: a **watchlist retention cap** (the maximum number of carried-forward worthy-but-unpicked names kept in active monitoring) and a **carry horizon** (the maximum number of runs a hypothesis is monitored without its leading metric confirming before it is retired). Both have generous defaults; retired nodes leave active monitoring but stay in run history for outcome-learning ([trade-opportunities.md §Discovery memory](trade-opportunities.md#discovery-memory-the-opportunity-graph)).

Portfolio Analysis has no equivalent setting: it grades a known holdings list and never screens the universe.

### Research Context Management (hierarchical distillation)

Both jobs consolidate per-item web research with one reusable primitive — *distill one complete research topic-tree into a structured object* — applied as a **single pass** when the research is small and **hierarchically** (tier-1 per topic-tree → a reduce) when it is large, so research can grow without overloading a single model call ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)). The choice is the **orchestrator's, made deterministically** from the accumulated evidence-ledger size — never the model's — and three knobs (generous, conservative defaults) bound it:

- **Distillation overflow threshold** — the fraction of a consolidation call's input budget above which the orchestrator switches from a single pass to hierarchical (leaving headroom for the thinking trace and the structured output). Applies to both jobs' per-item research (Trade Opportunities Step 5e, Portfolio Step 6d).
- **Heavy-route distinct-hypothesis count (*K*)** — Trade Opportunities discovery only: a route resolving to more than *K* distinct hypotheses is treated as **heavy** and sub-distilled along its seam before emitting cards ([trade-opportunities-workflow.md §Step 3b](trade-opportunities-workflow.md#step-3b-model-led-hypothesis-research)). A route also counts as heavy when it spans more than one **substantial sub-agenda**, defined deterministically: a side whose accumulated evidence-ledger size crosses a **configured per-side threshold** (the same evidence measure applied per side), with the **event-impact route's beneficiary / feared-loser / latent sides counted substantial whenever populated** — so "substantial" is never a model judgment.
- **Sub-distillation cap** — the maximum tier-1 sub-distillations a single item (a heavy route, or a large per-candidate / per-holding research set) may spend; beyond it the lowest-priority sub-units fail-soft to a recorded gap rather than overrunning. Sub-distillation is spent from the existing per-run discovery / per-item research budget (wall-clock binds first), and the chosen shape plus tier count are logged to the run's audit record, so the fan-out is never silent.
