# Big confirmation run — findings (2026-08-30, attempt 3, v9 full shape)

Findings from the single big confirmation run — the debut full-shape run of the
tunnel-vision `portfolio-v9`-era pipeline, stamped `portfolio-v30`, from a wiped
store so every holding is a debut, across a 47-position book.
The watch set it reads against is `big-run-watch-set.md`.

This record accumulates findings during and after the run, so some entries are
interim.
Where a finding rests on a single holding it is labelled as such and carries a
quantify-across-the-book action rather than a conclusion; a single holding is not
a rate.
Nothing here was changed while the run was in flight — every fix is deferred to
after the run, so adoption can read the run's own measured rates rather than a
one-holding impression.

## Run outcome — deliberately stopped early at 2 of 47

The run was cancelled by the user after two holdings (TSLA, PSX) completed —
`job_runs` id 4, state `cancelled`, no `portfolio_runs` record written, the store
otherwise the wiped debut state with the two holdings checkpointed.
It was stopped because Finding 1's failure mode materialized: under the research
loop's sustained volume the `google cse` engine began rate-limiting, so SearXNG
returned empty and queries spilled to the Tavily fallback (seven Tavily requests
logged in the Tavily console), and Tavily's quota is reserved for the
market-report job.
A re-attempt should apply Finding 1's mitigations — client-side pacing and engine
pruning, or a paid SERP overflow — before re-running, so the research loop does
not exhaust the report job's Tavily quota again.
Every finding below therefore rests on a two-holding sample and is labelled as
such; none is a rate.

## Run configuration

Dev app (`cargo run`, dev-scoped store) with the production corpus imported for
report continuity — 30 reports, 67 vector rows, 14 baseline snapshots — and the
portfolio store wiped to a clean debut (`prior_run_id = None`, zero prior runs,
checkpoints, episodes, or quick-checks).
The reasoner is `qwen3.5:122b-a10b` on the M5 Max, 100 % GPU, `num_ctx = 131072`
(the adapter's per-stage override, not the 256 K daemon auto-default), with
`OLLAMA_FLASH_ATTENTION=1`.
Web research runs SearXNG-primary via a freshly installed OrbStack; the operational
bring-up and its gotchas are recorded outside this file (the memory note
`searxng-orbstack-bringup`).
Header stamps observed: `portfolio-v30`, `checkpoint-v7`, `evidence-floor-v4`,
`grade-v2.3` — the shapes the watch set requires.

## Finding 1 — SearXNG engine blocking under the research loop

The first time the live per-holding research loop drove the self-hosted SearXNG
instance (`searxng/searxng:2026.8.22-9fea41204`, loopback `127.0.0.1:8888`,
`limiter: false`) at real volume, most engines were blocked upstream within
minutes.
Brave returned HTTP 429 and self-suspended for 180 s; DuckDuckGo, Qwant, and
Startpage returned CAPTCHA; Mojeek returned HTTP 403.
Despite this the aggregate queries kept returning 20–39 usable results, carried by
the `google cse` engine plus Qwant during its uncooled windows, and research
proceeded — pages were fetched and extracted from finance.yahoo.com, fool.com,
investors.com, techcrunch.com, simplywall.st and others (the extraction-quality
detail, including a ~19% thin-stub tail, is Finding 6).
The effective redundancy was thin from the first holding: `google cse` carried
almost every query while five of the six configured engines were already blocked,
so the Tavily spillover that ended the run was a question of when, not whether.

The failures are upstream, IP-reputation blocks by the engine operators, not the
instance's own rate limiter.
This distinction is load-bearing: the local limiter is already off
(`limiter: false`), so the large body of online "fix SearXNG 429" advice —
Valkey, `limiter.toml`, `trusted_proxies`, `X-Forwarded-For`, `pass_ip` —
addresses the local limiter and does not apply here.

### Two facts specific to this instance

The `google cse` engine is enabled and keyless here (`tokens=None`; no
`api_key`/`cx` configured), so there is no 100-requests-per-day Custom Search
JSON API quota to exhaust.
It is a scraped source that simply had not been IP-blocked yet, so it carries the
same could-be-rate-limited-under-volume risk as the others rather than a hard
quota cliff.

`use_default_settings: true` leaves far more engines enabled than the four the
shipped `searxng/settings.yml` names.
The running instance advertised 272 engines with a broad enabled set — `google
cse`, `google news`, `google scholar`, `reuters`, the Bing verticals,
`wikipedia`, `stackoverflow`, `arxiv`, and more — so the general-web redundancy
is wider than the shipped file implies.

### The safety net that made this a non-event

The web tool is SearXNG-primary with a Tavily fallback, and the fallback engages
on an empty result set, not only on a connection error
(`web_research/search.rs::search_routed`).
A SearXNG response that returns HTTP 200 with zero usable results after the
rank-time filter falls back to Tavily exactly as an error would, so a run where
every SearXNG engine is blocked still reaches the model with Tavily-sourced
evidence rather than an empty packet.
Each holding's research audit persists whether it used the fallback
(`tavily_fallback_used` on `portfolio::research::HoldingResearch` and
`portfolio::distill::ResearchAuditRecord`), so the run's own fallback rate is
recoverable per holding from the checkpoint rows.

### Mitigations, ranked, mapped to this project

The estimate that frames the choice: the loop fires on the order of dozens of
queries per holding across ~47 holdings, roughly 1,000–1,500 queries per run,
which is what overwhelms every IP-scraped engine.

1. **Pace and shrink the client-side query volume.**
   This is the highest-leverage keyless lever, because the blocks are triggered
   by burst rate from a single egress IP and SearXNG has no built-in inter-query
   delay.
   Concretely: hold concurrency to the endpoint at 1–2, space requests a few
   seconds apart with jitter, deduplicate and cache overlapping queries, and
   collapse the per-item query count from dozens to a handful.
   This is a change to the research loop's per-holding budgets
   (`MAX_FETCHES_PER_HOLDING`, `MAX_TURNS_PER_PASS`, `MAX_PASSES_PER_TOPIC`,
   `SEED_BUDGET_CHARS`), which the watch set deliberately kept as drafted until
   this run reads them — so it is a post-run calibration item, and this run is
   generating the volume and fallback data that should inform it.

2. **Prune the permanently-dead engines from `settings.yml`.**
   Dropping the engines that CAPTCHA on every call (Qwant, Startpage, the HTML
   Brave scraper) and keeping the ones that actually return (`google cse`,
   Mojeek, DuckDuckGo, Reuters) cuts per-query latency and log noise without
   adding a key, and fits the project's settled SearXNG-keyless-primary stance.

3. **Add a paid SERP API as an overflow fallback — only if the run's fallback
   rate proves costly.**
   SearXNG natively supports key-based engines immune to CAPTCHA (`google_cs`
   with `api_key`+`cx`, `braveapi`, `exaapi`, `yandex_api`), and a direct paid
   SERP API (Serper.dev at roughly $0.30–1 per 1,000) is what setups converge on
   for reliable automated search at volume.
   This conflicts with the project's decision to avoid a new paid search
   dependency, and Tavily already covers the overflow, so it is a revisit-later
   option gated on this run's measured Tavily cost, not a recommendation.

Skip two commonly-suggested paths.
Local-limiter tuning does not apply, since the limiter is already off and the
blocks are upstream.
Routing engines through Tor makes Google and DuckDuckGo block *more*
aggressively against exit-node IPs, so it worsens the major engines while helping
only niche ones.

For a keyless-preferred, cost-sensitive single-user setup the aligned path is
pacing plus engine pruning, with Tavily kept as the only paid fallback it already
is, decided after the run against the recorded per-holding `tavily_fallback_used`
rate and observed query volume.

## Finding 2 — Ledger `quant` under-population on numeric falsifiers

A model reasoning trace on the ledger-authoring call showed the model uncertain
about the ledger's JSON structure, deciding to "make it readable text" because
the prompt prose "doesn't explicitly define sub-schema for `ledger`."
The structural worry is unfounded, but the content consequence is real.

The `ledger` field is grammar-constrained, not free text.
`ledger_schema()` (`portfolio/mod.rs:2329`) is a fully structured JSON schema —
`falsifiers[]` and `triggers[]` each carry a `quant{ series (enum of the series
the engine computes), comparator (below/above), threshold, margin }`,
`key_drivers[]` carry a series enum, `family ∈ {add, trim, sell}`, every field
`required` — and it is embedded in the interpretation `format`
(`mod.rs:2508` priced, `mod.rs:2634` role-risk).
Ollama applies that grammar after the think block closes, so whatever the model
resolves to during thinking, the emitted object is forced into this shape, with
the app's Step-6g validator as defense in depth behind the constraint.
The persisted TSLA ledger confirms it: `verdict.thesis_ledger` decoded cleanly
into four structured conditions and two key drivers, both drivers backed by valid
engine series (`gross-margin`, `net-margin`), plus a populated `ledger_audit`.

The real signal is that the grammar guarantees structural validity, not content
completeness, and `quant` is nullable (`"type": ["object", "null"]`,
`mod.rs:2334`).
On two of TSLA's four conditions the model wrote a numeric falsifier into the
statement text — "Gross Margin < 16% sustained over two consecutive quarters" —
while leaving the machine-evaluable `quant` object null, even though gross-margin
is a computable engine series it used as a key driver.
Those conditions are structurally valid but semantically under-populated: the
threshold lives only in prose, so the engine cannot evaluate them for crossings
and they degrade to qualitative conditions on run 2.
The two near-duplicate gross-margin conditions also carry inconsistent
`technology_class` flags (one true, one false) on a plain financial metric that
is neither.

This is the "debut ledger authorship quality at 47-position scale" watch item,
and it rests on a single holding, so it is a candidate pattern, not a rate.
The quantify-across-the-book action: count, over every completed holding, the
conditions whose statement carries a numeric threshold while `quant` is null (a
mechanically-checkable falsifier degraded to prose), and the `technology_class`
mislabel rate.
Root cause is prompt-side: the prose under-specifies the `quant` sub-schema (the
schema is grammar-only) and likely the engine-series units/scale, so the model
plans prose thresholds rather than structured `quant` falsifiers.
The post-run prompt-calibration fix is to describe the falsifier/`quant`
structure and the series units in the prose so the model reasons about the
structured falsifiers it is actually authoring.

## Finding 3 — Action-call prompt friction

A reasoning trace on the per-holding action call surfaced two prompt-friction
signals, both low severity — the persisted output was clean.
For TSLA the model persisted `action = trim` (`action_source = model-chosen`),
the engine arm independently also chose `trim` (the two arms converged),
`action_annotations` was empty (no departure to record), and the one-sentence
rationale flagged the tax cost of realizing the gain without letting it move the
rung.

The first signal is that the ENGINE SET section made the model re-read it several
times, unsure whether an in-set pick needs a departure annotation and whether it
is told the engine's own pick.
The ground truth is that the departure annotation is app-stamped
(`portfolio/pipeline.rs:133`, the "action … outside the engine set … persisted
as authored" record), so the model never needs to emit it — it should just pick
its best rung on the full ladder.
The prompt does not say so, so the model spends reasoning re-deriving it; the fix
is to tell the prompt the app records departures.

The second signal is that the model reached for "capital efficiency" framing to
justify the trim while acknowledging the capital-efficiency read was
`indeterminate`, not `fails`.
The contract reserves capital-efficiency-as-exit-input for a `fails` read, so an
`indeterminate` read should stay neutral; the model did not clearly break the
rule — the F grade and negative base case independently justify the trim — but
the `indeterminate` read leaked in as a soft sell-lean, which is worth watching
for recurrence across the book.

## Cross-cutting note — thinking is a free side-channel

Findings 2 and 3 both come from the model's `<think>` block, which is an
ephemeral side-channel: Ollama applies the grammar-constrained `format` only
after the thinking token, and only the resulting JSON is persisted and validated,
so a meandering or uncertain trace cannot by itself produce malformed or invalid
output.
The signals worth acting on are therefore content-quality and prompt-clarity
signals for post-run calibration, not correctness bugs — the ledger `quant`
under-population is the one with real downstream cost, because it silently
narrows what the ledger can mechanically evaluate on later runs.

## Finding 4 — Fired bounded retries: per-holding rate and causes

The run tracker showed a "Local model retry — FAILED" row with the detail
"content failed its parse; retrying once: research findings response failed its
schema parse", followed by further successful local-model rows.
This is the bounded retry-once mechanism working as specified, not a defect.

Both of the first two completed holdings fired exactly one bounded retry, on two
distinct transient classes: TSLA on an `empty completion body` cause, PSX on a
`content failed its parse` cause, each persisted in the holding's `model_retries`
with its stage and cause — so the observability contract holds and the
per-holding retry rate is 2 of 2 so far.
This is a per-holding rate, not a per-call rate: each holding issues many model
calls (research turns, distillation, interpretation, action), so one retry per
holding is a low single-call failure rate surfacing once per holding at this
scale.
Both holdings completed with full verdicts, so the retries recovered on the
hard-path calls or were absorbed fail-soft on the research half — the safety net
is doing real work rather than sitting idle, which is itself the measurement: the
first-attempt failure rate is above the review's hoped-for zero, not at it.
The risk this raises for the rest of the run is a double failure on a hard-path
call — the interpretation or action call fails the holding after one retry
rather than degrading — with none seen yet.
The paragraphs below concern the `content failed its parse` case specifically,
because a grammar-constrained call failing schema parse is the more diagnostic of
the two causes.

The 6d research-findings terminal turn is grammar-constrained — `findings_schema()`
is passed as the `format` (`portfolio/research.rs:1101`) — and its returned
content failed to deserialize into `FindingsWire` (`research.rs:1127`).
A parse failure is a whitelisted transient class (`RetryClass::SchemaParse`,
`local_model.rs`), so the app re-issued the same turn exactly once and surfaced
it as the "Local model retry" tracker row plus a structured `model_retries`
event with stage and class.
The FAILED badge with the run continuing afterward means the re-attempt failed
its parse a second time and the research half absorbed it fail-soft — a findings
turn that fails even after its one retry degrades that pass to a thinner packet
rather than failing the holding (`research.rs:1145`).
This is the watch set's §Model serving and runtime item: the first live
measurement of the fired-retry rate the 2026-08-24 review could only bound by
construction, firing and being surfaced exactly as the contract requires.

The angle worth a post-run look is that this is a grammar-constrained call that
failed schema parse.
If the `format` grammar were fully engaging, the terminal turn's output would
deserialize by construction, so two consecutive parse failures point at either
the tools-plus-`format` edge where the grammar occasionally does not engage (the
report the M5 pre-flight found did not reproduce at 8/8, a small sample against
the hundreds of research turns here) or a truncated response whose incomplete
JSON body fails the parse.
The post-run dig is to correlate the `SchemaParse` retries against the
data-health truncation and context-pressure flags to tell truncation apart from
a grammar miss.
The exact recover-versus-degrade outcome per event and the run-wide retry rate
come from the persisted `model_retries` telemetry at run end.

## Finding 5 — Throughput: about 25 minutes per holding, a roughly 20-hour full run

Both completed holdings took about the same wall-clock time — TSLA about 24
minutes, PSX about 26 minutes, from a 16:21 run start — so the per-holding cost
is roughly 25 minutes and is not merely front-loaded on the first holding's
setup.
At that rate a full 47-position run is roughly 19–20 hours, a genuinely
overnight-plus single run that makes the checkpoint/resume design load-bearing
rather than a convenience.
The time is dominated by the per-holding research loop and its thinking-heavy
122B calls — research turns, distillation, interpretation, action — each on a
distinct large prompt; the Ollama log showed the prompt cache evicting
800 MB–1.2 GB entries between calls, so each call re-pays its prompt eval on a
long context rather than reusing a cached prefix.
This gives the Finding 1 pacing and query-reduction mitigation a second payoff:
cutting the per-holding query and fetch count would shorten the run as well as
spare the Tavily quota.

## Finding 6 — Extraction telemetry: first data for the deferred render tier

The run fetched 31 web documents across 23 distinct hosts over the two holdings.
Of those, 25 extracted cleanly and 6 were thin stubs (about 19%), and the
`extraction_quality` scores were bimodal — 19 documents at 1.0 with a tail
falling to 0.0, several below 0.2.
No host tripped the rendered-retrieval flag (`render_first = 0` for all 23),
which is expected since that tier is deferred and only its gating telemetry is
live.
This is the first live evidence for the deferred rendered-retrieval-tier decision
the watch set calls out: a non-trivial thin-stub and low-quality tail exists that
a render tier might rescue, but on 31 documents from two holdings it is a
direction to watch, not a rate — a full run is needed before the per-domain
thin-stub and `extraction_quality` rates can decide whether and where the tier
earns its slice.

## Metrics observed — two-holding sample (run stopped early)

The run was cancelled at two holdings, so these are observations from TSLA and
PSX, not rates; a full run is needed before any of them is trustworthy.

- Tavily fallback: 1 of 2 completed holdings — PSX fell back, TSLA did not.
  This spillover is what ended the run.
- Model-retry: 2 of 2 completed holdings fired one bounded retry — TSLA on an
  `empty completion body` cause (recovered; a full verdict landed) and PSX on a
  `content failed its parse` cause (the retry did not recover that findings pass;
  it was absorbed fail-soft and the holding still completed).
  Both events persisted in `model_retries` with stage and cause, so the
  observability contract held.
- Ledger `quant` population: TSLA authored two of its four conditions as numeric
  gross-margin falsifiers with `quant` null (Finding 2), with inconsistent
  `technology_class` across the two near-duplicates.
- Model-vs-engine action: TSLA converged — both arms chose `trim`,
  `action_annotations` empty — which a two-holding sample cannot turn into a
  divergence rate.

- Throughput: about 25 minutes per holding (both), projecting a full run at
  roughly 19–20 hours (Finding 5).
- Extraction: 31 documents across 23 hosts, 6 thin stubs (about 19%),
  `extraction_quality` bimodal (19 at 1.0, a tail to 0.0), zero render trips
  (Finding 6).

A future full run should recompute all six as real rates, and add the
`SchemaParse`-versus-truncation correlation that Finding 4 leaves open.

## Sources

- SearXNG `outgoing` settings (proxies, `using_tor_proxy`, retries, pools) — https://docs.searxng.org/admin/settings/settings_outgoing.html
- SearXNG `engines` settings (per-engine `timeout`, `retries`, `retry_on_http_error`) — https://docs.searxng.org/admin/settings/settings_engines.html
- SearXNG `search` settings (`suspended_times`, `max_page`) — https://docs.searxng.org/admin/settings/settings_search.html
- Shipped `settings.yml` — native key-based engines (`braveapi`, `exaapi`, `yandex_api`, `google_cs`) — https://github.com/searxng/searxng/blob/master/searx/settings.yml
- "Frustration with major engines" — SearXNG Discussion #5651 — https://github.com/searxng/searxng/discussions/5651
- Brave drops free Search API tier (metered billing since Feb 2026) — https://www.implicator.ai/brave-drops-free-search-api-tier-puts-all-developers-on-metered-billing/
- AI search API pricing (Tavily / Exa / Serper / SerpApi) — https://www.buildmvpfast.com/api-costs/ai-search
