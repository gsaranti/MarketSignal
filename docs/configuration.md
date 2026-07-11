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

The **Financial Modeling Prep**, **FRED**, and **Tavily** credentials are all required to run a **Market Signal Report** job:
- **Tavily** is the primary research and news-ingestion system, and news gathering is a mandatory workflow step (see [report-workflow.md §Step 7](report-workflow.md#step-7-gather-and-filter-news)).
- **Financial Modeling Prep** is the primary financial-data source and provides the equity-market portion of the baseline market-data scan — indices, volatility (VIX), gold, and sector performance (see [report-workflow.md §Step 3](report-workflow.md#step-3-gather-baseline-market-data)) — which is not optional, so a missing FMP credential blocks execution.
- **FRED** supplies the macro and commodity portion of the same baseline scan — the dollar index, oil, natural gas, and the 2Y/10Y Treasury yields — which is likewise not optional, so a missing FRED credential blocks execution. FRED requires a free API key on every request.

BLS, GDELT, and CFTC are also accessed through public APIs but need no Settings credential: **BLS** works keyless (an optional free key raises its rate limits), **GDELT** needs no key, and **CFTC** (the Commitments-of-Traders positioning source) is reached through the keyless Socrata public-reporting API.

If a required external provider credential (the Financial Modeling Prep, FRED, or Tavily credential) is missing:
- report generation is disabled
- the application displays a validation warning explaining which credential is missing
- the **local-suite jobs are blocked too when FMP or FRED is missing** — their execution gate shares those two credentials as presence preconditions, through this same warning category ([portfolio-workflow.md §Step 1](portfolio-workflow.md#step-1-job-start-and-gate)); Tavily does not gate the local suite, where it is an optional research fallback ([web-research.md §Tavily fallback](web-research.md#tavily-fallback))

For the data providers themselves and what each is used for, see [data-sources.md](data-sources.md).

## Local Analysis Suite Configuration

The local analysis suite (Portfolio Analysis and Trade Opportunities) is configured separately from the report, and its settings gate the **local jobs only** — they are independent of the report's execution gate, so a machine set up for one need not be set up for the other (with two shared exceptions: the **FMP and FRED** data credentials above, which both gates require as presence preconditions — [portfolio-workflow.md §Step 1](portfolio-workflow.md#step-1-job-start-and-gate)).

### Local Models

The suite calls a local model daemon — Ollama, over its native HTTP API (the call path, and why it is not the daemon's OpenAI-compatible layer, is specified in [local-models.md §Serving runtime](local-models.md#serving-runtime)). Settings hold:
- the **daemon endpoint** (the local Ollama URL)
- the **model roster** — the model ids for the **reasoner** and the **embedder** (both required), plus an **optional fast tier** (benchmark-gated; see [local-models.md](local-models.md) for the recommended defaults)

The local-suite Run buttons are **locked** whenever the daemon endpoint or a **required** roster id (the reasoner or the embedder — the optional fast tier never gates) is unset — a presence check, exactly like the cloud model selectors and data-source tokens above, raising the persistent **local models not configured** warning until they are filled. Whether the daemon is actually **reachable** and the rostered models actually **pulled** is a separate, connectivity check verified only at the **run-gate** and by a **manual *Test Connection*** here — not probed at startup (see [local-models.md §Serving runtime](local-models.md#serving-runtime), [interface.md §Connection status](interface.md#connection-status-local-suite)). When a connectivity check finds the daemon or a rostered model missing, it offers **guided setup** — installing Ollama (deep-link / Homebrew) and pulling the roster from inside the app, with `pull` progress on the run tracker; the app supervises a user-installed Ollama, it does not bundle it.

### Web Research

The suite's web-research tool uses a local SearXNG instance, with the existing Tavily credential as a fallback (see [web-research.md](web-research.md)). Settings hold the **SearXNG endpoint**; no key is required for the local instance, and the Tavily fallback reuses the credential already configured above. The user self-hosts the instance from a **pinned `docker-compose.yml` the app ships** — the load-bearing JSON-output / bot-limiter config baked in — so setup is one command; when the instance is unreachable or misconfigured the **Settings connection row** deep-links to that setup and to a Docker / **OrbStack** install, and the pre-run notice flags the degraded run (SearXNG is never a warning-area category — see [web-research.md §Search backend](web-research.md#search-backend-searxng), [interface.md §Pre-run web-research notice](interface.md#pre-run-web-research-notice-local-suite)). The suite ships a **default source registry** — per-domain evidence tiers over a heuristic floor ([data-sources.md §Source registry and evidence tiers](data-sources.md#source-registry-and-evidence-tiers)). A **user-facing override surface** (pinning a domain's tier or adding it to the deny list) is **deferred**: the registry is a thin override most domains need no entry in, so hand-tuning — and the settings-store schema it would need — waits until a real need appears.

### Connected Sources (subscriptions)

An **optional** feature for connecting the user's own paid subscriptions (WSJ, FT, The Economist, Morningstar, specialist research) so the web-research loop can reach paywalled content the public web can't ([web-research.md §Connected sources](web-research.md#connected-sources-authenticated-fetch)). Settings list each connected source with its **health state** (`connected` / `connected_but_thin` / `expired` / `unsupported`) and a connect / re-login control; the domain-scoped session material lives in the **macOS Keychain**, like the Schwab credentials, never in the SQLite settings store. **Connected Sources is never part of the execution gate** — a local job runs with none connected; a lapsed (`expired`) source is surfaced for re-login and treated as absent until refreshed, never blocking a run.

### Price Data

The suite's price and fundamentals load is spread across keyless providers (see [data-sources.md](data-sources.md)). **SEC EDGAR** and **Stooq** are keyless and need no configuration; live **quotes** come from the shared FMP key. Per-stock **option chains** come from the Schwab connection (below), and **CBOE**'s venue-level put/call backdrop is keyless — neither needs separate configuration.

### Charles Schwab Connection

Both local jobs source data from Charles Schwab via OAuth — Portfolio Analysis its holdings, and both jobs the options-activity signal from option chains (see [schwab-integration.md](schwab-integration.md)). Settings hold the developer **app key and secret** and manage the **connection state** (connect / re-authenticate); the app secret and the OAuth tokens are kept in the **macOS Keychain**, not the SQLite settings store, since they are bearer credentials to the brokerage account. **A connected Schwab account is required to run either local job** — it is part of the local-job execution gate, alongside the model daemon and roster. Because the OAuth refresh token expires every 7 days, both jobs are blocked with a re-authentication prompt when it lapses. Manual import can supplement holdings but does not satisfy the connection gate.

### Investor Profile

Both local jobs are personalized by an **investor profile**: risk tolerance, time horizon, objective, tax sensitivity, and cash posture. It shapes Portfolio Analysis's action ladder and cash/deployment stance — **never the intrinsic verdict**: grade, sub-scores, conviction, targets, and the standalone lean are **profile-independent**, computed identically for any investor, the profile entering at portfolio construction only ([portfolio-analysis.md §Intrinsic verdict](portfolio-analysis.md#intrinsic-verdict)) — and Trade Opportunities' entry framing and conviction emphasis (see [trade-opportunities.md](trade-opportunities.md)). The profile never changes *which* holdings grade well or *which* opportunities qualify — those are engine and research outputs — only how the prescription is framed for this investor.

**For now the profile is a fixed default preset, not user-configured** (a configurable profile is deferred). Settings surfaces the preset **read-only** — its values are shown so the investor posture shaping every action is visible, but not yet editable. The default posture:

- **horizon — long-term.** The job favors durable multi-quarter / multi-year theses over short-term trades.
- **objective — maximize profit.** Total return is the goal; no income or capital-preservation mandate is imposed.
- **risk tolerance — medium-to-high.** Higher-risk cells and archetypes (disruptors, commodity cyclicals, smaller caps) are in scope, gated by the engine's forensic/risk discipline rather than by a conservative cap.
- **cash — always available.** Buying power is treated as **unconstrained**: the user may hold cash in accounts the app can't see, so *add aggressively* and full-size entries are **never** gated on observed Schwab cash. (Concentration and risk limits still apply; only the cash constraint is lifted.)
- **tax sensitivity — no precise modeling.** No tax-lot, holding-period, account-type, or marginal-rate calculation is applied to actions — the job never computes a tax harvest. It does treat the **possible tax benefit of realizing a loss** as one *generic, qualitative* counterweight once a position's forward prospects have already been judged poor, weighed beside the redeployment value of freed cash ([portfolio-analysis.md §Portfolio action](portfolio-analysis.md#portfolio-action)); it is framed as *possible* precisely because account type and rate are unmodeled, so the user judges their own specifics.

These defaults are the stated posture the suite runs against until a configurable profile exists.

### Trade Opportunities — Discovery Breadth

Trade Opportunities runs as **two jobs** ([trade-opportunities.md §The two jobs](trade-opportunities.md#the-two-jobs)); only **Discover (DTO)** has a compute setting. DTO is a discovery funnel: it screens the whole universe, then spends expensive per-candidate validation (deep local-model research plus per-symbol data) only on a narrowed set ([trade-opportunities-workflow.md §Step 4](trade-opportunities-workflow.md#step-4-candidate-consolidation)). Because that per-candidate work runs on a local reasoner, one run cannot deep-research every surfaced name — so a single **deep-research budget** caps how many names DTO deep-researches per run, split three ways: a **reserved rotation slice** (a configured share, default ~20%, floored at one slot) spent first on live opportunities in **maintenance-priority order** (warning-bearing → catalyst-near → threshold-near → stalest, with a **max-age service level** force-promoting any name whose research age exceeds the bound — the matrix's bounded self-refresh, so no live name's research can age indefinitely), then **brand-new candidates**, with any **leftover** spent on existing opportunities that re-surfaced through discovery (oldest-deep-researched first); every other live opportunity gets only the engine-only cheap re-derivation that run. The **rotation share and its max-age service level** are themselves configurable (0% disables the self-refresh, restoring purely user-directed re-research). Settings expose this budget as a **discovery-breadth control**: a generous default, raised for a more exhaustive (longer) run or lowered for a faster one. **Audit (ATO)** has no compute setting — its depth is bounded by the opportunities the user selects (a large Deep-Audit selection confirms first).

This is a **compute budget, not a quality cap**. At the point it applies nothing has been validated yet, so a name that doesn't fit the budget is **not rejected, only deferred** — and a genuinely worthy deferral (a real hypothesis + an identified leading metric) is **remembered**: written to the persisted **opportunity graph** as a watchlist node and re-checked every later run with its leading metric monitored, so a deferred name that quietly compounds is caught rather than lost to chance ([trade-opportunities.md §Discovery memory](trade-opportunities.md#discovery-memory-the-opportunity-graph)). (Only the *validated* opportunity matrix and this discovery graph carry forward through storage; a name not even worth watchlisting carries no state and is simply re-derivable by a later sweep.) The budget is always spent under the **diversity guardrails** (market-cap band / feeder / archetype / sector-theme), so a wider or narrower setting still can't collapse the funnel onto mega-cap momentum or one crowded theme. Crucially, the **final matrix has no output cap**: every candidate that clears the validation gates is listed, ranked by conviction ([trade-opportunities.md §The opportunity space](trade-opportunities.md#the-opportunity-space)) — the budget bounds only what gets *researched* per run, never how many *good, validated* ideas are shown.

Two **discovery-memory** settings bound that watchlist so it can't grow without limit: a **watchlist retention cap** (the maximum number of carried-forward worthy-but-unpicked names kept in active monitoring) and a **carry horizon** (how long a hypothesis is monitored without its leading metric confirming before it is retired — counted in the metric's own reporting periods, not runs, so the bound doesn't depend on how often the user runs DTO). Both have generous defaults; retired nodes leave active monitoring but stay in run history for outcome-learning ([trade-opportunities.md §Discovery memory](trade-opportunities.md#discovery-memory-the-opportunity-graph)). A third, calibration-side bound is the **shadow-ledger retention cap** — the maximum number of active turn-away **decision episodes** (the ledger's entry unit, one per ticker per turn-away — Step-5h rejects, dedup-collapsed peers, retired / unpromoted watchlist nodes) kept in label tracking for the price-only shadow scorecard — when the cap binds, the **oldest active entries are evicted before their labels mature**, logged as a coverage loss (the generous default makes this the pathological case, not the norm) — each surviving entry frozen into a compact matured archive (its own cap) once its 12-month outcome labels have been recorded ([trade-opportunities.md §Outcome learning](trade-opportunities.md#outcome-learning-calibration)).

Portfolio Analysis has no equivalent setting: it grades a known holdings list and never screens the universe.

### Research Context Management (hierarchical distillation)

Both jobs consolidate per-item web research with one reusable primitive — *distill one complete research topic-tree into a structured object* — applied as a **single pass** when the research is small and **hierarchically** (tier-1 per topic-tree → a reduce) when it is large, so research can grow without overloading a single model call ([web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)). The choice is the **orchestrator's, made deterministically** from the accumulated evidence-ledger size — never the model's — and four knobs (generous, conservative defaults) apply here: the first three bound the single-vs-hierarchical distillation choice, the last bounds the volume of model-attributed seed provenance:

- **Distillation overflow threshold** — the fraction of a consolidation call's input budget above which the orchestrator switches from a single pass to hierarchical (leaving headroom for the thinking trace and the structured output). Applies to both jobs' per-item research (Trade Opportunities Step 5e, Portfolio Step 6d).
- **Heavy-route distinct-hypothesis count (*K*)** — Trade Opportunities discovery only: a route resolving to more than *K* distinct hypotheses is treated as **heavy** and sub-distilled along its seam before emitting cards ([trade-opportunities-workflow.md §Step 3b](trade-opportunities-workflow.md#step-3b-model-led-hypothesis-research)). A route also counts as heavy when it spans more than one **substantial sub-agenda**, defined deterministically: a side whose accumulated evidence-ledger size crosses a **configured per-side threshold** (the same evidence measure applied per side), with the **event-impact route's beneficiary / feared-loser / latent sides counted substantial whenever populated** — so "substantial" is never a model judgment.
- **Sub-distillation cap** — the maximum tier-1 sub-distillations a single item (a heavy route, or a large per-candidate / per-holding research set) may spend; beyond it the lowest-priority sub-units fail-soft to a recorded gap rather than overrunning. Sub-distillation is spent from the existing per-run discovery / per-item research budget (wall-clock binds first), and the chosen shape plus tier count are logged to the run's audit record, so the fan-out is never silent.
- **Seed-lineage cap** — the maximum number of model-attributed **`seeded_by`** seeds a research stage may record beyond those captured deterministically (a `surfaced_by` tag is stamped free whenever a seed's URL is deep-read and needs no knob; this caps only the seeds the reasoner *names* as orienting a hypothesis without fetching them — most visibly a Trade Opportunities discovery hypothesis card, [web-research.md §The research loop and context management](web-research.md#the-research-loop-and-context-management)). A small default, so seed provenance can't bloat a card or the model's working context.
