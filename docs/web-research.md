# Web Research Tool

The local analysis suite reaches the open web through one tool — **search, fetch, and extract** — that the Rust orchestrator runs on the model's behalf.
A stage requests a search or a page; the application layer performs the network I/O and returns clean text.
The model never touches the network, holding the same pure-stage boundary as the report pipeline (see [local-models.md](local-models.md), [agents.md](agents.md)).
The tool is **keyless, local-first, and cost-free** by default.

## The research loop and context management

A research stage runs as a bounded, multi-turn loop.
The stage's **agenda** — the specific topics the research must answer for the item under study — is **assembled deterministically by the orchestrator** from that stage's documented topic list (fixed topics plus deterministically triggered conditional ones, e.g. Portfolio's technology-event topic); the reasoner (the 122B model in thinking mode) *works* the agenda, never authors it, **one topic at a time**: each topic gets its own focused **research pass** over a clean context (the dossier facts, that topic's questions, and any orienting **reuse seed** the job's contract defines — e.g. Portfolio's per-topic prior distilled object, [portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
One carve-out: Trade Opportunities' discovery routes have no documented topic list, so there the Step-3b planning call **proposes each route's topics and the app validates them** like the route list itself — still a model-proposes / app-validates agenda, never the research model authoring topics mid-loop ([trade-opportunities-workflow.md §Step 3b](trade-opportunities-workflow.md#step-3b-model-led-hypothesis-research); ruled 2026-08-19).
A pass is itself a bounded multi-turn tool loop — the model emits `web_search` / `web_fetch` calls, the orchestrator executes them and returns the results as tool messages for the next turn, until the topic is answered or the budget (below) is spent.
When a pass surfaces a sub-thread worth pursuing, it may spawn an **app-governed follow-up pass**, bounded to **depth ≤2** — a topic's root pass plus at most two follow-ups, so **≤3 passes per topic**.
That cap counts passes (branches), *not* raw LLM turns: the turns and fetches inside each pass are governed by the per-item budget below, not the depth cap.
A follow-up is the model's *proposal*, carried as a structured field the orchestrator reads and decides whether to spend; the model never recurses on its own.
**Terminology:** a topic is worked as one **isolated conversation** — its pass loop; where a workflow doc says *one call per topic*, it means exactly this isolated per-topic conversation, within which each **turn** is one model request and the orchestrator owns every tool execution — never a single-request contract.

The orchestrator — not the model — owns every request, so the loop is bounded the way the report's research executor is.
Two ceilings work together: the per-topic depth cap above (a quality guard against rabbit-holing one topic) and a **per-item budget that binds first** — a cap on **web-fetch attempts and wall-clock per item** (a failed live attempt spends like a served one, so failing URLs can't ride for free; a document-cache hit spends nothing), spent across all topics in priority order and polled at each request boundary (see [report-workflow.md](report-workflow.md)).
That boundary poll is a *between-requests gate*, never a mid-request kill — a model call or fetch already in flight always runs to completion.
A spent budget then stops further fetches, any follow-up pass, and the move to the next topic, but does not suppress the current pass's one terminal findings turn.
That turn emits findings rather than requesting tools — the rule the per-job logic flows already state, *once the topic is answered or the budget is spent, emits that pass's findings* — so a budget-interrupted pass still yields findings, not nothing.
A genuinely hung request is still abandoned by a *separate* per-call stuck-daemon timeout, so honoring in-flight completion never hands a non-responding call an unbounded lease.
When the budget drains, the lowest-priority remaining topics are skipped fail-soft (recorded as a degraded-input gap, lower conviction), never failing the run.
The model decides *what* to look up; the application decides *how much* it is allowed to.
The fetch-count, topic, and depth caps are pinned defaults; the wall-clock cap is calibrated against measured local throughput on first runs.

**Context stays bounded by extraction and an evidence ledger — not by re-distilling findings mid-loop.**
Local models have finite context, so the orchestrator never hands the model raw pages or a long turn-by-turn transcript: each fetched page is **readability-extracted** to its article text, and every extracted claim is appended to a per-item **evidence ledger** (claim + source URL + timestamp).
Within a topic's bounded pass the reasoner works over that topic's extracted page text and the ledger; as-built the pressure is bounded at insertion — each fetch's text is capped before it enters the context, and turns and claims per pass are capped — while a pressure-driven roll-off of older raw page text (the ledger retaining its claims, so nothing would be silently dropped) remains an unbuilt refinement.
Crucially, the loop does **not** run an in-between model step that re-summarizes the model's own findings — each topic's pass emits its **full findings response**, preserved whole and carried (with its ledger entries) to the **distillation stage downstream**.
That per-topic response is **assembled deterministically** — the orchestrator accumulates each pass's findings as distinct, ledger-linked entries across the topic's ≤3 passes (append-only, structure preserved so a heavy route can later sub-distill along its seam), **never a topic-close model synthesis** — so the **first** model consolidation of the findings, for any stage, is the downstream call (per-candidate distillation, or discovery's card formation).
That is the deliberate division of labor: the only mid-loop reduction is the deterministic page extraction a finite context forces, the model's reasoning output is never re-distilled *during the research loop*, so research is never planned over *this run's* lossy, already-summarized notes *and* distillation sees each topic's complete context (a persisted *prior*-run distilled object seeding a later loop, where a job's reuse contract defines it, is a distinct cross-run mechanism outside this in-loop rule).
The loop stops when the agenda's questions are answered or the budget is spent; the per-topic full findings — each claim carrying its source URL and timestamp — are what flow to that downstream distillation (the "forward only what's needed" rule from [local-models.md §Context-memory discipline](local-models.md#context-memory-discipline) is applied **after** research, not inside it).

**Seeds that orient a loop are recorded as leads, distinct from evidence.**
Some research loops are *seeded* by a structured feed — a stage opens a topic primed with ticker-tagged, dated headlines that point at what to pursue (most visibly Trade Opportunities' discovery routes, seeded by the FMP news / articles feeds and the macro-release calendar — [trade-opportunities-workflow.md §Step 3b](trade-opportunities-workflow.md#step-3b-model-led-hypothesis-research)).
A seed is a **lead, not a citation**: it orients the research, but the conviction-bearing content must still come from the web tool's deep-read of the underlying source, so a seed is **never written into the evidence ledger as a claim** — that would re-admit a thin, second-hand snippet as if it were verified evidence.
Each loop instead keeps a small, typed **seed-lineage lane** beside the ledger, populated two ways: **(1)** *deterministically* — whenever the model deep-reads a seed's URL, the resulting ledger claim carries a **`surfaced_by`** back-pointer to that seed (its feed source, headline, URL, timestamp), so "what oriented this finding" is recoverable at ~no cost; and **(2)** *model-attributed* — a stage may name the bounded set of seeds it judges to have shaped its hypothesis even where it did not fetch them, capped by config so seed provenance can't bloat the working context ([configuration.md §Research Context Management](configuration.md#research-context-management-hierarchical-distillation)); the as-built Portfolio loop keeps the first **4 distinct known ids per pass** and gap-logs duplicates, unknown ids, and over-cap ids.
Because model-attributed lineage is the one **fabricable** leg, it is **validated, not trusted**: the orchestrator assigns every seed it feeds a loop a **stable seed ID**, a `seeded_by` entry must reference one of that loop's known IDs, and the app **drops and logs any unknown reference** — so the reasoner can attribute among the *real* seeds but cannot invent one (the same model-proposes / app-validates split the hypothesis score uses).
Distillation reads the seed-lineage lane as **provenance, never as scored evidence**; a useful by-product is that the gap between a seed's claim and what its deep-read actually found is itself a narrative-vs-reality signal.
The lineage rides the stage's structured output into the run audit record — and, in discovery, onto the opportunity-graph node ([trade-opportunities.md §Discovery memory](trade-opportunities.md#discovery-memory-the-opportunity-graph)).

**The downstream distillation may be hierarchical.**
Consolidation is one reusable primitive — *distill one complete research topic-tree (a top-level topic plus its ≤3 looped passes) into a compact, structured object* — applied wherever a single consolidation call would overflow the model's working context.
When the per-item research is small the stage is a **single pass** over every topic-tree's full findings, exactly as before.
When it is large (many topics, or a deep tree), the stage becomes **map-reduce**: a **tier-1** distillation per topic-tree, then a **tier-2 reduce** over those tier-1 outputs into the one object the next stage reads.
This lets research grow without any single call overloading, and lets the model focus on a coherent subset at a time.
Two invariants keep it faithful to the no-mid-loop-re-distillation rule above: **(1)** the tier-1 distillation of a topic-tree covers that tree's **complete** findings (never an in-loop summary) — sub-distilling along its **pass seam** (a pass-level map — each pass carrying its findings *and* its ledger-linked entries, each map call counted against the sub-distillation cap — then a tree-level reduce, which is not counted) only where one tree's **complete** tier-1 input (its findings, their evidence-ledger entries, and any job-specific input such as Portfolio's reuse prior) would itself overflow a single call — and runs only *after* that tree's research is done, so research is still never planned over *this run's* distilled notes; and **(2)** any reasoning that must span topics — most importantly Trade Opportunities' **cross-lens contradiction check**, which only bites once all lenses share a context ([trade-opportunities.md §Reconciling the lenses](trade-opportunities.md#reconciling-the-lenses-the-contradiction-check)) — lives at the **tier-2 reduce**, the first place the trees meet, with each tier-1 output a **structured, field-preserving** reduction (per-lens claims with their sources and confidence, plus any internal-tension flag) so nothing the reduce depends on is lost.
The orchestrator decides single-vs-hierarchical **deterministically** — the consolidation call's **full input size** (every topic's findings and the accumulated evidence ledger, plus whatever job-specific inputs join them — Portfolio's per-topic reuse priors, Trade Opportunities' engine reads) against its input budget — never the model; the thresholds are config knobs ([configuration.md §Local Analysis Suite Configuration](configuration.md#local-analysis-suite-configuration)).
That routing sizes the content.
The rendered single-pass and tier-1 prompts are then sized once more against the widest budget the adapter can issue — the reasoner's where a distinct fast tier is resident, the same budget otherwise — and one that outgrows it — the content sum omits the instruction scaffolding — takes the next smaller shape rather than issuing: hierarchical for the single pass, that topic's pass-seam sub-distillation for a tier-1 call.
The adapter seam then sizes each rendered consolidation prompt at issue, routing up to a wider resident model or refusing before any request exists ([local-models.md §The local-model adapter seam](local-models.md#the-local-model-adapter-seam)).

## Search backend: SearXNG

Search is served by a **self-hosted SearXNG** instance running locally and queried over its JSON API on the loopback interface.
SearXNG is a metasearch front end: it fans a query out to real engines, parses and merges the results, and returns structured hits (title, URL, snippet) the orchestrator can rank and fetch.

**Rationale (SearXNG over a paid search API):**
- **cost-free, no per-query credit ceiling** — a deep multi-step research loop over many items can't exhaust a metered quota (though upstream engines can still rate-limit or CAPTCHA individual queries — see Failure posture);
- **local-first** — no API key and no paid-service dependency in the default path (the local instance still queries public engines, but you don't rely on any single provider's API);
- **engine diversity** — results aren't bound to one engine's ranking or rate limits.

**The user self-hosts the instance; the app ships its configuration.**
SearXNG is a Python web app, so the app does **not** bundle or supervise the server process — vendoring a Python runtime and its native-extension tree would inflate the binary and the macOS signing surface the design keeps flat (the same stance that has the render tier reuse the embedded webview rather than bundle a second browser — see [§Fetch and extraction](#fetch-and-extraction)).
Instead the app ships a **pinned `docker-compose.yml`** (a fixed image tag, never `latest`) plus a mounted **`settings.yml`** that bakes the load-bearing configuration in, so first-run setup is one command (`docker compose up -d`) and the config is never a manual step.
Two settings travel in that `settings.yml` and are load-bearing: SearXNG's **JSON format is disabled by default** (an unset format returns HTTP 403), so JSON output is enabled; and the **bot limiter is disabled** for the single-user loopback instance (it exists to protect a *public* instance from bots, which a private local one doesn't need).
The shipped `settings.yml` also curates the keyless engine set against live-run observations ([verification/2026-08-30-big-run-findings.md §Finding 1](verification/2026-08-30-big-run-findings.md)), enabling the general engines that serve and disabling the ones that CAPTCHA or rate-limit on effectively every automated call.
The set is deliberately widened for redundancy rather than pinned to a curated few, because which engines block flips day to day — the ones that carry one run can all be blocked the next.
`use_default_settings` still supplies engine breadth beyond that curated list, and the engine names take effect only when the instance runs, so they are confirmed by the health-check probe at bring-up rather than at app build.
The engine set is cleanup, not the rate fix: the upstream engines block on **burst rate from a single egress IP**, which is an upstream IP-reputation limit rather than SearXNG's own already-disabled bot limiter, so the load-bearing rate defense is client-side in the search layer.
The search layer paces consecutive SearXNG queries to a minimum interval with jitter, polled at each query boundary, and the first query of a run is never delayed.
Only back-to-back queries within a single model turn actually wait, because a full reasoning turn already spaces queries far past the interval, so the added wall-clock is negligible against the thinking-dominated per-holding cost.
The search layer also caches results by normalized query (case and spacing folded; punctuation preserved, since SearXNG treats some of it as operators) for the run, so a repeated query across a topic's passes returns without re-hitting the engines; the cache is deliberately conservative — a false hit would feed the wrong results, a false miss costs only one paced query — so it matches only queries that are textually identical bar case and spacing.
The pacing interval is a calibratable default.
The per-holding research budgets are left as drafted, to be calibrated against a full run's measured query volume rather than changed by this slice.

**A paid SERP engine (Serper) is the keyless set's reliable floor.**
Pacing reduces the burst that trips the blocks, but it does not end the fight with Google's bot detection: the IP-reputation blocks escalate under the loop's sustained volume and persist across days, so a keyless-only set can go dark mid-run with no recovery guarantee.
Serper is a paid Google SERP API that queries Google from its own infrastructure — immune to the egress-IP blocks — wired **inside** SearXNG as a `json_engine`, a keyed engine alongside the keyless ones exactly as Brave's API would be, so the app stays SearXNG-only and unchanged.
It fires on every query as the floor while the keyless engines remain enabled as zero-cost bonus redundancy, at roughly $0.30–1 per thousand queries against a query-heavy run (a free tier covers early validation).
The key never enters the repo: `settings.yml` carries only a `${SERPER_API_KEY}` placeholder, and the real key is rendered from an out-of-repo secrets file into a gitignored runtime settings file at bring-up through a gitignored compose override, so the tracked template holds no secret.

The app **health-checks** the running instance — the same health-check *mechanism* it uses for the model daemon (see [local-models.md §Serving runtime](local-models.md#serving-runtime)), but **never on the execution gate**: an unreachable SearXNG degrades the run (the local suite is SearXNG-only, so research thins toward blind and Trade Opportunities discovery yields fewer candidates — [§Tavily fallback](#tavily-fallback)), it does not block it.
The check drives a **connection-status indicator in Settings** ([interface.md §Connection status](interface.md#connection-status-local-suite)) that deep-links to the setup when SearXNG is down or misconfigured; the install pointer recommends **OrbStack** on Apple Silicon over Docker Desktop (lighter, no commercial-licensing question).

**Alternatives weighed (app-bundled SearXNG; Brave's API).**
Both were considered and parked, and the reasons are load-bearing.
*App-bundling the SearXNG server* is dominated: for a technical, Docker-capable user the shipped compose file gives the same engine breadth without the Python footprint, and if the app ever wanted to own search end-to-end the clean path would be a **Rust-native metasearch** (more Rust, no second process, the footprint stance intact), not a vendored Python tree — bundling earns its cost only for non-technical distribution where Docker is a non-starter (and where the app would be notarizing anyway).
*Brave's Search API* would remove self-hosting entirely (a keyed HTTPS call, no Docker), but it reintroduces precisely what SearXNG is chosen to avoid — a **metered per-query ceiling** against deliberately query-heavy local research loops, plus single-provider dependency and lost engine diversity — so Brave stays a **documented contingency** (the paid backend to reach for *only* if self-hosting is ever abandoned, preferred over Exa), not the default.
Brave already contributes results *inside* SearXNG as a keyless, unmetered engine; only its API adds the key and the meter.
Adding a *keyed engine inside* SearXNG — as Serper now is for Google — is the middle path taken above: it buys one provider's reliability without replacing the metasearch or taking on a single-provider ceiling.

## Fetch and extraction

Search returns links; the tool then **fetches the top results and extracts readable text**.
The fetch is a plain HTTP GET carrying a **realistic, browser-like header set** (a current User-Agent plus the coherent `Accept` / `Accept-Language` / `Sec-Fetch-*` headers a real browser sends) and a timeout — header hygiene is **cheap prevention on the default path**, so the common fetch isn't needlessly flagged as a bot before the render tier is ever reached (it won't fool fingerprint-based detectors, where a non-browser TLS handshake still gives it away — that's the render escalation's job, not the GET's); extraction strips navigation, ads, and boilerplate down to the article body so the model reasons over content, not page chrome.
Readability extraction is done in Rust (a `readability.js`-style article extractor).
Pages that are paywalled or render their content with client-side JavaScript return thin text to a non-browser fetch — a fetch-layer limit, not an extractor failure — and such results simply contribute less evidence rather than breaking the loop.
**That thin-text case is the trigger for an optional *rendered-retrieval* tier — a selective escalation, not a new default.**
The plain GET stays the default for the bulk of fetches; only pages the extraction telemetry flags as thin escalate to a **render fetch** that executes the page's JavaScript before extraction, recovering a body a non-browser GET can't. The render reuses the **browser engine the app already embeds** (the Tauri webview) rather than bundling a second browser (Playwright / Selenium) or a Python scraper sidecar (Crawl4AI) — keeping the binary footprint and the macOS signing surface flat; an external headless browser stays a **spike-gated fallback** for a publisher the embedded webview can't drive.
A render fetch holds the same safety posture as the plain GET ([§Safety and provenance](#safety-and-provenance)) and feeds the same `extractionQuality` telemetry, so escalation stays **measured, never blanket** — browser-rendering every fetch would be slow and heavy, so it fires only on the flagged subset.

## Tavily fallback

The local suite has **no Tavily fallback**: Tavily is **reserved for the Market Signal Report job**, whose research and news ingestion it serves ([data-sources.md](data-sources.md)), so no local job may spend that quota.
The local research tool therefore runs **SearXNG-only** — when the local SearXNG instance **can't serve search** (unreachable, misconfigured with JSON output disabled, or returning nothing), a local run does **not** reach for Tavily.
The search returns empty or failed and the research loop **fail-softs to a thinner packet** (lower conviction on the affected item), consistent with the suite's honest-degradation stance ([§Failure posture](#failure-posture)).

**Portfolio Analysis** enforces this by construction: it wires no Tavily backend into its web tool, so a local run can never spill onto the report's reserved quota — the failure mode a shared fallback created.
**Trade Opportunities** inherits the same posture when built — its discovery lane was already SearXNG-only, and its per-candidate validation is SearXNG-only too (a paid overflow, if ever wanted, is a separate key or SERP API, never the report's Tavily key).

When a **web-research run** is launched with SearXNG unavailable — Portfolio Analysis (full or selective), Trade Opportunities **Discover (DTO)**, or an **ATO Deep Audit** — the app surfaces a **pre-run notice** confirming this degraded mode before spending the run, job-specific (the *fewer-candidates* consequence is DTO-discovery-specific; a Deep Audit only loses validation depth on its selected names).
Because there is no fallback, a degraded run researches **blind**, so the notice is always flagged *not recommended*, but it is never a block.
**The engine-only paths — ATO's Quick Audit and Portfolio's Quick check — do no web research, so they never trigger this notice** ([interface.md §Pre-run web-research notice](interface.md#pre-run-web-research-notice-local-suite)).

## Safety and provenance

Because the model chooses what to fetch, fetching is treated as an untrusted operation:

- **SSRF protection.**
  Fetches are restricted to `http`/`https` and to public hosts — private, loopback, link-local, and the other special-use ranges (carrier-grade NAT, protocol-assignment, benchmarking, TEST-NET / documentation, reserved space, and the deprecated IPv6 site-local block) are blocked (this matters specifically because the app's own Ollama and SearXNG run on loopback), redirects are capped and re-validated against the same rules, and responses are bounded by size and content type (HTML/text only).
  A literal address — IPv4 or bracketed IPv6 — is validated as itself with no lookup and pinned exactly as a resolved name is.
  A public literal therefore fetches, and a non-public one is blocked with its reason.
  A **cached document re-passes the URL policy** (scheme, deny list, literal-address rules) on both its requested-URL key and its stored post-redirect final URL before it may serve, so an imported or legacy cache row cannot bypass the current source policy.
- **Untrusted content.**
  Fetched page text is data, not instructions: it is inserted into the prompt as quoted evidence and never interpreted as a directive, so a page carrying injected instructions cannot redirect the analysis.
- **Provenance.**
  Every research finding carries its **source URL and retrieval timestamp**, so a verdict or opportunity can be traced to what it was based on and when — feeding the run's audit record (see [portfolio-analysis.md](portfolio-analysis.md), [storage.md](storage.md)).

## Source quality and evidence weighting

The web loop reaches an unbounded set of domains, so the suite weighs *where evidence comes from*, not only *what it says*.
This is the **engine-computes / model-interprets** spine applied to sourcing: the **app computes objective source signals** and the **model interprets content with those signals in view**, against the per-domain metadata in [data-sources.md §Source registry and evidence tiers](data-sources.md#source-registry-and-evidence-tiers).

**The load-bearing rule: source quality informs, it never gates.**
A low-tier or thinly-sourced finding **lowers conviction**; it does **not** remove a candidate or claim from consideration.
This governs the **evidence tiers (0–5)** — a low tier weights down, never excludes; the **one exception is the explicit `deny` policy**, which drops junk that isn't evidence at all (SEO mills, AI-generated quote pages, PR-spam) at search-filter and fetch-gate ([data-sources.md §Source registry and evidence tiers](data-sources.md#source-registry-and-evidence-tiers)) — excluding a non-source isn't gating *on quality*, it is keeping spam out of the evidence base (**tiers grade, `deny` excludes, nothing between is gated**).
The rule's scope is **source quality** — the tier gradient and its soft annotations (`recencyScore` against `freshnessSlaDays` stays a weight, never a gate).
It does **not** govern *claim freshness*: whether a floor-bearing input (a leading metric, bear-case evidence, statements) is current enough to gate on is a **job-owned evidence-floor question**, decided by Trade Opportunities' typed freshness basis ([trade-opportunities.md §Starting parameters](trade-opportunities.md#starting-parameters-calibratable)) — the deny-style carve-out for age, deliberately narrow so `freshnessSlaDays` can never silently turn from a weight into a gate.
This is deliberate and protects the job's edge — Trade Opportunities' whole value is surfacing **under-covered, early-inflection names no structured feed carries** ([trade-opportunities.md §The pipeline](trade-opportunities.md#the-pipeline)), and those names live in Tier-3/4 coverage by definition; gating discovery on source tier would systematically suppress exactly what the job hunts.
The discipline mirrors the suite's existing stance — positioning held out of the grade until calibrated, the since-flagged read kept cap-only — quality is a **conviction input, never a survival gate**.

**The evidence annotation, split by who can know it.**
Each fetched document is annotated, and the split is strict:
- **App-computed (deterministic):** `sourceTier` (from the registry / default heuristic), `extractionQuality` (0–1, how much real article body the readability pass recovered vs a thin paywall / JS stub), `recencyScore` (0–1, against the source's `freshnessSlaDays`), `primarySourceBonus`, and a paywall / JS-stub flag.
- **Model-derived (judgment):** `claimSpecificity`, `contradictionFlag`, and which claim IDs a document supports — these are reasoning, so they stay model-side and are never dressed up as app-computed.

The model sees the app's source-quality judgment **alongside** the text and weighs evidence accordingly — a Tier-0 filing outweighs a Tier-4 blog on a reported number, while a Tier-3 specialist outweighs a Tier-2 generalist on its own vertical (the `evidenceKinds` match).

**Lane policy.**
The same source is treated differently by lane:
- **Discovery** — **soft preference**, never a hard floor: breadth is the point, so a promising lead from a low-tier source is still pursued, only weighted down.
- **Per-candidate / per-holding validation** — **stricter weighting**: a name's verdict should rest on higher-tier corroboration, and a claim resting only on Tier-4/5 sources is flagged low-confidence (still surfaced, with the gap recorded — informing, not gating).
- **Portfolio** leans to primary filings, company IR, transcripts, and material-event reporting; **Trade Opportunities** leans to specialist and value-chain sources.
  These are the per-route source strategies of [trade-opportunities-workflow.md §Step 3b](trade-opportunities-workflow.md#step-3b-model-led-hypothesis-research), now expressed through the registry's `evidenceKinds`.

**Source-diversity / syndication caps.**
Five outlets reprinting one Reuters wire are **one** independent source, not five.
The loop collapses near-duplicate and same-canonical-origin hits so apparent corroboration can't be inflated by syndication — independence is counted by *origin*, not by *URL count*.

**A disconfirming-fetch pass.**
Beyond the existing bear case and adversarial passes ([trade-opportunities.md §The research method](trade-opportunities.md#the-research-method)), once a thesis is formed the loop spends one bounded pass searching specifically for **what would disprove it** — a disconfirming *fetch*, not just a disconfirming *prompt*.
It is **spent from the existing per-item / per-route fetch + wall-clock budget, never added on top of the ceilings** ([§The research loop and context management](#the-research-loop-and-context-management)): a high-priority item *within* that budget that **fail-softs to a recorded gap (and lower conviction) when the budget is already exhausted**, so a thesis is tested against contrary evidence before it earns conviction without ever breaching the loop's hard bound.
Portfolio's placement of the pass — per holding, after its topics — is specified at [portfolio-workflow.md §Step 6c](portfolio-workflow.md#step-6c-bounded-web-research).
Trade Opportunities' placement — per candidate, after its Step-5d topics — is specified at [trade-opportunities-workflow.md §Step 5d](trade-opportunities-workflow.md#step-5d-bounded-web-research).

**Extraction telemetry.**
The fetch layer tracks, per domain, how often it recovers full article text vs a thin paywall / JS stub (the same telemetry stance as the report's document-truncation tracking).
This feeds two things: a domain's `extractionProfile` in the registry, and the **health test** that decides whether a connected subscription is actually yielding value (below) — so a source that renders poorly through a non-browser fetch is never silently trusted as if it did.

## Connected sources (authenticated fetch)

Much of the highest-value financial content is paywalled (WSJ, FT, The Economist, Morningstar, specialist research), and **the user's own subscription is the one way to reach it that even a paid search API can't** — Tavily can't read the user's WSJ login.
**Connected Sources** is an **optional enrichment feature, never part of the execution gate**: a local job runs fine with none connected; connecting a source only deepens the evidence available.

**The flow rides the Schwab credential rails** ([schwab-integration.md](schwab-integration.md)):
1. The user adds a source (WSJ, FT, Economist, Morningstar, SemiAnalysis, …);
2. the app opens a **dedicated in-app login window** for that domain (the user authenticates normally, including any SSO / 2FA);
3. the app stores **only the minimum domain-scoped session material** in the **macOS Keychain** (a bearer credential, like the Schwab tokens — never in the SQLite settings store);
4. the fetch layer then attaches that session for that domain, so an authenticated GET returns full article text instead of a paywall stub;
5. the app runs a **source health test** and records a state.

**Health-test states** — because authentication defeats the paywall but not client-side rendering, an authenticated source is only as good as its measured extraction yield:
- **`connected`** — search finds it, fetch retrieves full text, readability recovers enough body;
- **`connected_but_thin`** — authenticated but the content renders poorly through a non-browser fetch (JS-heavy), so it yields little; **down-ranked accordingly, not silently trusted**;
- **`expired`** — the session lapsed; the source is surfaced for re-login (like Schwab's 7-day re-auth) and treated as absent until refreshed;
- **`unsupported`** — the domain can't be made to yield through any fetch path, **rendered tier included** — even a full JS render recovers no usable body.

The state conditions the registry's `tier` / `lanePolicy` for that domain: a `connected_but_thin` source does not rank as a full Tier-2/3 source just because it is paid.

**Rendered retrieval promotes a thin authenticated source.**
Because `connected_but_thin` is a *rendering* failure, not an *auth* failure, it is the prime case for the rendered-retrieval tier ([§Fetch and extraction](#fetch-and-extraction)): the app drives its **already-authenticated webview** — the same one the user logged in through, so the session and cookies are already established — to the target URL, lets it render the JavaScript, and extracts the resulting DOM.
A source that yields full body this way is **re-tested and promoted `connected_but_thin → connected`**, earning back the registry `tier` / `lanePolicy` rank it was held out of.
Reusing the embedded webview pays double here: one fetch defeats **both** the paywall (it carries the login session) **and** client-side rendering (it runs the page's JS).
The operational caveat below still binds — a publisher may fingerprint or invalidate the session regardless — so promotion is **yield-gated per fetch, never assumed from the fact of a login**, holding the existing rule that a poorly-rendering paid source is not silently ranked high.

**Spend guidance** (which subscriptions earn their keep — the same *pay-for-information-not-analysis* logic): **The Economist** for the macro / regime worldview; **Morningstar** when the portfolio holds funds / ETFs or for moat / fair-value context; **one or two vertical sources matched to actual exposure** (SemiAnalysis for semis / AI-infra, STAT / Endpoints for biotech, Platts / Argus / Wood Mackenzie for energy / materials); **FT or WSJ** if already subscribed — not assumed the highest marginal edge over primary data plus Reuters / AP-style factual reporting.
A subscription is only worth its rank where the **health test shows real extraction yield**.

Authenticated fetch holds the same safety posture as the rest of the loop ([§Safety and provenance](#safety-and-provenance)): SSRF guards still apply, fetched content is still data-not-instructions, and every finding still carries its source URL + timestamp.
The one honest caveat is operational and outside the app's control — automated access to a subscription can run against a publisher's terms and sessions can be invalidated server-side, so Connected Sources is **best-effort enrichment**, never a guaranteed source.

## Failure posture

Web research is **fail-soft**.
A failed search, a timed-out fetch, or an empty result degrades the evidence for the item under study; it does not fail the run.
The model proceeds with whatever evidence landed, and the thinner evidence is reflected in the analysis (for example, lower conviction), consistent with the suite's honest-degradation stance (see [local-models.md §Failure posture](local-models.md#failure-posture)).
