# Web Research Tool

The local analysis suite reaches the open web through one tool — **search, fetch, and extract** — that the Rust orchestrator runs on the model's behalf.
A stage requests a search or a page; the application layer performs the network I/O and returns clean text.
The model never touches the network, holding the same pure-stage boundary as the report pipeline (see [local-models.md](local-models.md), [agents.md](agents.md)).
The tool is **keyless, local-first, and cost-free** by default.

## The research loop and context management

A research stage runs as a bounded, multi-turn loop.
The stage's **agenda** — the topics the research must answer for the item under study — is **assembled deterministically by the orchestrator** from that stage's documented topic list: fixed topics plus deterministically triggered conditional ones, such as Portfolio's technology-event topic, which the engine's event pre-flag alone makes eligible.
The reasoner (the 122B model in thinking mode) *works* the agenda, never authors it, **one topic at a time**: each topic gets its own focused **research pass** over a clean conversation.
One carve-out: Trade Opportunities' discovery routes have no documented topic list, so there the Step-3b planning call **proposes each route's topics and the app validates them** like the route list itself — still a model-proposes / app-validates agenda, never the research model authoring topics mid-loop ([trade-opportunities-workflow.md §Step 3b](trade-opportunities-workflow.md#step-3b-model-led-hypothesis-research)).
A pass is itself a bounded multi-turn tool loop — the model emits `web_search` / `web_fetch` calls, the orchestrator executes them and returns the results as tool messages for the next turn, until the topic is answered or the budget (below) is spent — followed by a separate synthesis conversation that writes the pass's **write-up**, the topic's running account of what its research established.

**The gathering brief.**
Portfolio's gathering brief is one message in two parts on the frame every Portfolio prompt shares: the data with its glosses, then the task.
Part 1 leads with the text that is constant across the holding's topics and ends with the topic's own.
The constant text is the holding header with the analysis date; the fetched values the holding's dossier carries, as [portfolio-workflow.md §Step 6c](portfolio-workflow.md#step-6c-bounded-web-research) lists them; the news leads; on a continuity run the prior analysis and the prior thesis document; and, on a topic or follow-up pass, the pages already retrieved while researching this holding.
The topic's text follows: the topic's questions; on a follow-up pass the question it pursues and the topic's write-up so far; on the disconfirming pass the run's write-ups so far.
The order serves the runtime's prompt cache: consecutive topic conversations on one holding share their leading text byte for byte, and the topic text after it stays short enough to sit past the previous conversation's saved checkpoint ([local-model-operations.md §Serving & memory](local-model-operations.md#serving--memory-apple-silicon-128-gb)).
A truncated or re-read page shortens that shared prefix, and the saving is an expectation of the runtime until a run's serve log confirms it.
Part 2 states what to find, how to weigh a source (a source tier nearer 0 and an extraction quality nearer 1 preferred; a weak source lowers confidence, it does not exclude; a figure that cannot be right is a defect of the source), the per-reply tool-call bound, and when to stop — a reply with no tool call.
Where pages are shown, it asks the model to read them before searching for the remaining answers; a brief with no page shown asks to search first.
The model judges what the shown pages leave unanswered; there is no question-status tool and no app-assigned answered list.
The follow-up pass's opening states what to find on the follow-up question, then that the topic's questions are what that question serves and are not searched on the pass.
The disconfirming pass's opening states what to find on its question, then that the write-ups are what that question tests, searched for evidence against them and not for more evidence for them.
The fetch clause names the news leads as candidates beside the search results, under the one test of what is most likely to answer the questions.
The gathering system message names the two tools its verbs map onto, `web_search` and `web_fetch`.
The two tool descriptions state what a search result and a fetched page carry, with the tier scale's range, 0 to 5, beside its endpoints; the fetch description states the extraction-quality range, 0 to 1, and the stub flag in plain words, and carries no weighing or safety instruction.
Each fetched page is shown as `web_fetch` returns it: a header stating the address, the title, the publication date the search reported, the retrieval time, the source tier, the subjects the source is trusted on, the extraction quality and the stub flag, then the page's text framed as quoted material.
The tool results are data in the same register — a search result's fields, a page's header and quoted text, and a failed search or fetch stated as such — with no instruction in any of them.
A tool result never carries the operator's error text.
A failed search returns one fixed sentence, "SEARCH FAILED: the search did not complete."
A failed fetch returns one of five, chosen by the failure's typed class: the site's HTTP answer with its status, an address the app does not fetch, a page that could not be read, an invalid address, or no answer, each ending "No text was retrieved."
A remembered failure replays the class its live failure had, so the line reads the same either way.
The raw error text rides the run tracker's request row, where it is already recorded.
A search whose results all fall to the rank-time filter is an empty answer, rendered "No results." and counted as an empty search, not a failed one.
A call the app cannot read returns "ERROR: unknown or malformed tool call …" with the name it saw.

**Gathering is bounded before every request, never by the server's own truncation.**
One gathering response may request at most **8 tool calls** (`MAX_TOOL_CALLS_PER_TURN`): the orchestrator executes the deterministic head, records the omitted tail as partial coverage, and moves directly to synthesis rather than silently continuing after an over-large batch.
Before every gathering model call, the orchestrator serializes the complete growing message history plus the tool schema and refuses to issue the request if it would cross the shared input-budget guard (with a fixed request-envelope reserve); it applies the same check before retaining each assistant tool-call turn and tool result, then stops gathering and records any omitted result or unexecuted call as degradation when the next addition would overflow.
The same message-and-tool serializer feeds the run's prompt-size telemetry, so assistant tool calls and the tool schema cannot disappear from the likely-front-truncation read merely because their visible message content is empty.
Search titles, snippets, dates, and display URLs plus fetched-page titles, dates, and display URLs are capped or omitted in gathering tool results, so untrusted metadata cannot consume the packet independently of the page-text cap.
Before each gathering request the app appends a short user message stating the replies remaining in this pass, including the current reply (8 down to 1), and that pages fetched on the last reply are kept, since the loop executes the last reply's tool calls before gathering ends.
The initial brief, the reuse block and every previously issued message stay unchanged; each new countdown follows the initial brief or the completed tool-result batch.
The initial allowance reserves the first countdown once, and the full serialized input guard counts every accumulated countdown alongside the other messages.
A bounded transport retry repeats the identical packet and count without appending again, and a new pass starts at 8.

**Pages already retrieved on the holding are reused before another search is requested.**
Ordinary topic and follow-up gathering begins with a bounded selection of raw pages already retrieved while researching this holding.
The transient holding-scoped inventory retains capped page text together with the title, publication date, original retrieval timestamp, source annotation, requested/final URL lineage and truncation status; a successful document-cache read can populate it, but failed reads and empty bodies cannot supply reused evidence.
Selection follows first-retrieval order, rechecks current URL policy on the requested and final addresses, and fits complete page framing plus usable body text into the initial-message allowance after reserving the questions, task and first countdown message.
Unselected pages are counted in a plain omission note and persisted gap; shortened bodies carry the continuation marker and a gap, and the complete serialized gathering packet still passes the input guard before issue.
No network attempt or extraction-telemetry sample is charged for this in-memory reuse.
The inventory is discarded between holdings and is never persisted; an explicit successful re-fetch replaces a source's content and provenance together while preserving its requested aliases and first-retrieval position.
Other topics' write-ups and transcripts are never supplied to a topic pass; the disconfirming pass alone carries the run's write-ups, and it keeps its dedicated contrary-search behavior without automatic page injection.

**The synthesis conversation writes the pass's write-up.**
The write-up is authored by a **separate synthesis conversation** — a fresh conversation carrying the gathered evidence but no tool-call history and no tools, so a tool-using gathering turn and the write-up never share one request — in thinking mode, with no output grammar: the write-up is prose.
Its first message is one message in two parts on the shared frame.
Part 1 leads with the holding header and the fetched values, then the evidence, and ends with the topic's own text: the topic's questions; on a follow-up pass the question it pursues and the topic's write-up so far; on the disconfirming pass the write-ups it tests.
Consecutive syntheses on one holding can then share their header, fetched values and leading pages byte for byte, under the same cache expectation as the gathering brief.
The evidence is every page the pass's gathering conversation carried — the reused pages in first-retrieval order, then the pages the pass requested in fetch order, deduplicated by final URL, an explicit re-read of a reused page keeping that page's position with the fresh read's provenance — each under the header `web_fetch` gave it, the header fields glossed once above the pages.
The evidence planner jointly selects source headers and body allocations: every selected source must fit its header, fixed markers, and usable body text, omitted headers are reclaimed before the surviving bodies are water-filled, and every truncation or omission is shown inline and persisted as a gap.
Explicitly requested pages keep first claim on that budget: under overflow a reused page's tail is cut before an explicit page loses text.
Part 2 is the task: the write-up states what the research established on the topic's questions, quotes each figure with the date or period its source gives for it and names that source — the address of the page that states it, or the fetched values where the figure comes from them — says where pages disagree, and says what the evidence leaves unanswered, within a length band stated as an instruction ([portfolio-analysis.md §Starting parameters](portfolio-analysis.md#starting-parameters-calibratable)).
On a follow-up pass the model rewrites the topic's write-up whole, folding the new evidence into what the write-up so far established, so a topic has one write-up at any time.
The app reads the write-up as text and validates nothing in it — whether it cites faithfully is the model's own doing — and an empty body is the adapter's transient class ([local-models.md §The local-model adapter seam](local-models.md#the-local-model-adapter-seam)).
The second message of the same conversation asks whether the research has a follow-up question.
The reply is the question as plain text and nothing else, or the one word `none`, which is the only reply the app interprets: anything else is the follow-up question verbatim, and it becomes the next pass's question.
The second message is not sent on a topic's last pass under the depth cap, since no pass could take up its question, nor on the disconfirming pass.
A pass that retrieved no page with body text spends no synthesis conversation: the topic's write-up so far stands, the pass leaves none of its own, and its losses persist as gaps.
The degradation the gathering history carried — failed or empty searches, failed fetches, a per-page fetch truncated at the text cap, capped or budget-skipped calls — is recorded as a persisted gap and reaches no model: the synthesis never saw which search or fetch served which question, so the write-up states what the evidence leaves unanswered without a reason.
Those persisted per-holding research gaps are counted into `DataHealth.research_degraded_holdings` and `research_gap_count`, and the Portfolio roll-up summary renders the result; the counts remain informational rather than an attention trigger because research is additive and fail-soft.

**Follow-ups are app-governed and bounded by depth.**
When a pass asks a follow-up question, it spawns an **app-governed follow-up pass**, bounded to **depth ≤2** — a topic's root pass plus at most two follow-ups, so **≤3 passes per topic**.
That cap counts passes (branches), *not* raw LLM turns: the turns and fetches inside each pass are governed by the per-item budget below, not the depth cap.
A follow-up is the model's *question*; the orchestrator reads it and decides whether to spend it, and the model never recurses on its own.
Portfolio schedules passes by root-before-follow-up priority, then agenda order, then depth: every eligible root precedes pending follow-ups, and each topic's follow-ups retain its priority through the depth cap.
Technology-topic eligibility is settled when the agenda is assembled, from the engine's event pre-flag; a follow-up question cannot add a topic mid-loop.
Each pass begins a clean conversation with the holding-constant text, its own topic's text and write-up so far, and the bounded holding-scoped page reuse.
Budget exhaustion records skipped roots separately from unspent follow-ups; roots themselves may exhaust the budget, so the ordering does not guarantee complete coverage.
The disconfirming pass runs last, after the topic work, under the same budget: one gathering conversation that searches for evidence against the run's write-ups so far, then its own synthesis conversation, which writes its write-up and asks no follow-up ([portfolio-workflow.md §Step 6c](portfolio-workflow.md#step-6c-bounded-web-research)).
**Terminology:** a topic is worked in **isolation** from other topics — its own pass loop over a clean conversation, with Portfolio's bounded raw-page reuse but no other topic's write-up or transcript, plus each pass's separate synthesis conversation; where a workflow doc says *one call per topic*, it means this per-topic isolation — within which each **turn** is one model request and the orchestrator owns every tool execution — never a single-request contract.

The orchestrator — not the model — owns every request, so the loop is bounded the way the report's research executor is.
Several ceilings work together: the per-pass turn, per-turn tool-call, and aggregate-history bounds above; the per-topic depth cap (a quality guard against rabbit-holing one topic); and a **per-item budget that binds first** — a cap on **web-fetch attempts and wall-clock per item** (a failed live attempt spends like a served one, so failing URLs can't ride for free; a document-cache hit or remembered failure spends nothing; a retry spends another attempt), spent across all topics in priority order and polled at each request boundary (see [report-workflow.md](report-workflow.md)).
That boundary poll is a *between-requests gate*, never a mid-request kill — a model call or fetch already in flight always runs to completion.
A spent budget then stops further fetches, any follow-up pass, and the move to the next topic, but does not suppress the current pass's synthesis.
That synthesis is the separate conversation over the gathered evidence, not a tool turn — *once the topic is answered or the budget is spent, the pass's write-up is authored* — so a budget-interrupted pass still yields a write-up, not nothing.
A genuinely hung request is still abandoned by a *separate* per-call stuck-daemon timeout, so honoring in-flight completion never hands a non-responding call an unbounded lease.
When the budget drains, the lowest-priority remaining topics are skipped fail-soft (recorded as a degraded-input gap, lower conviction), never failing the run.
The model decides *what* to look up; the application decides *how much* it is allowed to.
The fetch-count, topic, depth, turn, and per-turn tool-call caps are pinned defaults; the wall-clock cap is calibrated against measured local throughput on first runs.

**Context stays bounded by extraction and by bounded documents — never by a transcript.**
Local models have finite context, so the orchestrator never hands the model raw source documents or an unbounded turn-by-turn transcript: each fetched page is **readability-extracted** to its article text and capped, and each topic's research accumulates in one bounded document, its write-up.
Within a pass the reasoner works over the pages its conversation carries and the write-up so far; pressure is bounded by the per-result field and page-text caps, the 8-call per-turn cap, the turn cap, and the aggregate gathering-packet guard before every request, while a pressure-driven roll-off of older raw page text remains unnecessary and unbuilt.
The only reductions inside the loop are the deterministic page extraction a finite context forces and the model's own rewrite of its topic's write-up over the new pages.
Research is never planned over a distillate: the gathering brief carries pages and the write-up so far, never a shortened copy of either, and the first consolidation across topics is the downstream analysis (below).
The loop stops when the agenda's questions are answered or the budget is spent; the topics' write-ups are what flow to that consolidation (the "forward only what's needed" rule from [local-models.md §Context-memory discipline](local-models.md#context-memory-discipline) is applied **after** research, not inside it).
Provenance rides the page and its roster, not a typed claim: every page the model reads carries its address, the publication date the search reported and the retrieval time in its header; the write-up is instructed to name the source of each dated fact — the page that states it, or the fetched values where the fact comes from them — and whether it does so faithfully is the model's own doing; and the app persists on the holding's audit record the roster of pages shown to the model, each with its address, title, publication date, retrieval time and source tier, never its text ([storage.md](storage.md)).

**Leads orient a loop; they are not evidence.**
Portfolio's gathering brief names the holding's news leads — dated headlines with their addresses — as fetch candidates beside the search results.
A lead is a lead, not a citation: it points at what to pursue, and a write-up rests on the pages the model read, never on a lead's headline or snippet.
Trade Opportunities' discovery routes carry their leads the same way — the FMP news and articles feeds and the macro-release calendar — and keep nothing of a lead: a hypothesis document rests on the pages the route read ([trade-opportunities-workflow.md §Step 3b](trade-opportunities-workflow.md#step-3b-model-led-hypothesis-research)).

**Consolidation distills write-ups, and only when the analysis prompt is over budget.**
After a holding's research, the analysis call reads the topics' write-ups, the fetched values and, on a continuity run, the prior analysis ([portfolio-workflow.md §Step 6d](portfolio-workflow.md#step-6d-consolidation)).
Before it issues, the orchestrator sizes that prompt against the call's input budget.
Within budget, the write-ups go in as written.
Over it, the merged write-ups are distilled into one shorter document; where the merged write-ups themselves exceed what one distillation call can take, each write-up is distilled first and the merge of those outputs is distilled again.
The prior analysis is never distilled: it is the holding's research memory, and the write-ups are what this run can afford to shorten.
Because the write-ups and the analysis each carry a length band, the check has a bound to work against; without one it would distill the write-ups to nothing to make room for a growing analysis.
The shape is the orchestrator's choice, made deterministically from the input's size against the thresholds in [configuration.md §Research Context Management](configuration.md#research-context-management); the model never chooses.
Each distillation call runs in non-thinking mode with no output grammar and returns prose.
The adapter seam sizes each rendered distillation prompt once more at issue, routing an over-budget prompt up to a wider resident model or refusing it before any request exists ([local-models.md §The local-model adapter seam](local-models.md#the-local-model-adapter-seam)).
The chosen shape and call count are logged to the run's audit record, so a distillation is never silent.
The analysis is the only research artifact the next run reads; the write-ups persist on the audit record as written, never as distilled.

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
The shipped `settings.yml` also curates the keyless engine set against live-run observations, enabling the general engines that serve and disabling the ones that CAPTCHA or rate-limit on effectively every automated call.
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
For `sec.gov` and its subdomains, the User-Agent is instead the declared app identity shared with the SEC EDGAR adapter, including its contact information.
The fetcher selects that identity from each redirect hop's parsed destination host, treating a terminal DNS root dot equivalently; other destinations retain the browser User-Agent.
This header does not imply successful access: an HTTP 401 or 403 still follows the denial policy below.
Readability extraction is done in Rust (a `readability.js`-style article extractor).
Pages that are paywalled or render their content with client-side JavaScript return thin text to a non-browser fetch — a fetch-layer limit, not an extractor failure — and such results simply contribute less evidence rather than breaking the loop.
**That thin-text case is the trigger for an optional *rendered-retrieval* tier — a selective escalation, not a new default.**
The plain GET stays the default for the bulk of fetches; only pages the extraction telemetry flags as thin escalate to a **render fetch** that executes the page's JavaScript before extraction, recovering a body a non-browser GET can't. The render reuses the **browser engine the app already embeds** (the Tauri webview) rather than bundling a second browser (Playwright / Selenium) or a Python scraper sidecar (Crawl4AI) — keeping the binary footprint and the macOS signing surface flat; an external headless browser stays a **spike-gated fallback** for a publisher the embedded webview can't drive.
A render fetch holds the same safety posture as the plain GET ([§Safety and provenance](#safety-and-provenance)) and feeds the same `extractionQuality` telemetry, so escalation stays **measured, never blanket** — browser-rendering every fetch would be slow and heavy, so it fires only on the flagged subset.

### Portfolio earnings-release recovery

Portfolio can recover an identifiable current-holding earnings-results release from SEC EDGAR after an issuer-host HTTP 403, including a remembered denial or another URL suppressed by that denial's host cooldown.
This is partial recovery: it does not implement rendered retrieval, recover peer issuers or paywalled commentary, interpret production/consensus pages, or extract figures that exist only in images.
The same existing FMP profile read supplies the corporate website; the exact corporate host, its `www` host and its `ir`, `investor` or `investors` subdomains are eligible, with both the requested and denying destinations required to belong to that set.
The website is an eligibility hint, not proof that two documents are equivalent.
The existing ticker-to-CIK resolver supplies the issuer identity, and the submissions response must independently agree on CIK and ticker.
All identity and discovery state is transient and stays out of prompts, checkpoints and portability archives.

The requested URL path or a bounded search/seed title must explicitly identify earnings results and one quarter/year; conflicting periods, opaque requests without an identifying title, announcement notices, preliminary/corrected results and unsupported intent remain unavailable.
Conflicts within either lead, including partial quarter/year information, cannot be overridden by the other lead; titles or paths naming an announcement date remain ineligible.
“Year-to-date” within quarterly results is eligible period wording, including in an “announces results” title; a separate announcement/release-date notice remains ineligible even when it also contains that wording.
A year printed in a URL is read literally, never shifted to the previous year to manufacture a fourth-quarter match.
An available search/seed publication date bounds candidate discovery to seven days on either side; that date is a lead, never publication evidence for the recovered release.
Without a dated lead, only an issuer whose submissions metadata explicitly declares a December 31 fiscal year end can use the requested calendar quarter end through the following 100 days as its discovery window.
An explicit fiscal label requires a dated lead and matches literal source wording without calendar conversion.
Fiscal matching requires a compact fiscal quarter/year label directly associated with results, with no competing fiscal period or quarter; a section containing guidance, outlook, forecasts or projected/expected results remains unresolved even if it also mentions actual results.
The current submissions response is the history boundary; additional historical files are not traversed.

The resolver considers Item 2.02 8-K filings in that window and later 8-K/A amendments, which remain plausible beyond the ordinary window.
More than four plausible filings leaves the match unresolved before document retrieval.
The filing's typed document table must identify one Exhibit 99.1 within the same issuer/accession, and the primary 8-K's Item 2.02 must link that exhibit as the first exhibit associated with a press/earnings release and identify the requested reporting period.
The parser unwraps only the SEC's exact inline-XBRL `ix?doc=` link form; it does not render that viewer.
Filing dates and event/report dates never substitute for the reporting period.
Multiple matching filings, a matching amendment/correction, malformed or unavailable discovery documents, or a request limit reached before every plausible candidate is checked leave the match unresolved.
Correction and preliminary-result checks cover the full Item 2.02 section, including wording after the exhibit reference; only recognized Securities Act/Exchange Act “as amended” boilerplate is exempt.
The selected exhibit must yield identifiable issuer/quarter financial text through the existing extractor; an index, cover page or image-only body cannot count as recovered earnings evidence.

Each resolution permits at most ten additional live attempts, including retries, inside the existing 40-fetch and elapsed-time holding limits.
The runner checks cancellation and both budgets before each discovery or exhibit request and applies the existing single transient retry policy.
SEC GETs are paced, including redirect hops, and retain the shared declared identity, URL policy, address pinning and response bounds.
Discovery accepts bounded SEC JSON/HTML separately from extracted pages; discovery metadata never enters the evidence roster or the successful-page extraction profile.
Failed live discovery attempts retain normal source-failure telemetry.
Submissions metadata and completed resolution outcomes are reused only within the holding, and issuer/release deduplication prevents repeated denied URLs from launching the same resolution again.
The original IR denial and its cooldown are retained; recovery diagnostics distinguish that failed address from the actual SEC source.

A recovered exhibit passes through ordinary quoted-page rendering, page/body limits, holding-scoped reuse and shown-source citation admission in the same pass.
Its document cache key and citation are the SEC address, never the denied IR URL or a fabricated redirect alias.
Publication provenance comes only from a recognizable dateline in the release's own text and otherwise remains unknown; filing/event dates and the failed URL's search metadata are not copied into that field.
The retrieval timestamp is the exhibit's original retrieval time, including document-cache service.
The renderer's wording and persisted shapes are unchanged by this route.

### Failed fetch memory and bounded retry

Failed-fetch memory is in-process and shared across every holding and topic of one Portfolio invocation, including the disconfirming passes.
A fresh invocation, including resume, starts empty; none of this memory is stored in `web_source_state`, checkpoints or portability archives.
A URL key is its parsed URL with the fragment removed, preserving path case, trailing slashes and query strings; a host key is the exact lowercase host, keeping `www` and other subdomains separate.
Current URL policy is checked first, and usable cached documents can serve before failure memory or host cooldown is consulted; cached redirect destinations still re-pass policy.

An HTTP 401 or 403 records the requested and denying URLs and puts the **denying host** into a five-minute cooldown, checked before contacting any redirect destination as well as on direct requests.
During the window, repeated URLs return the recorded failure and other URLs on that host are skipped; neither refreshes the expiry nor spends a live attempt.
Expiry permits a new probe; another denial starts a new five-minute window.
A redirect chain that already contacted an earlier host still spends its one admitted attempt when a later hop is suppressed.
The requested alias then remembers that failure until the denying host's original expiry, so repeating the alias makes no request and spends no further attempt.
The denial's host identity is deliberately separate from persisted failure telemetry, which retains requested-host attribution.
Other deterministic HTTP/document failures are remembered for the invocation; policy refusals are rechecked without contacting the source.

Only HTTP **408, 429, 500, 502, 503 and 504**, typed timeouts, and positively identified connection resets, connection aborts or broken pipes qualify for **one application-managed retry after one second**.
An opaque DNS, TLS or other connection error is unclassified, not guessed transient from its message; an unclassified failure gets no automatic retry and its URL becomes eligible again after 30 seconds.
After a transient failure exhausts its retry, its URL likewise has a 30-second cooldown, after which a new attempt plus one retry is eligible.
A retry that instead returns a denial or deterministic failure adopts that failure's policy.
The retry checks cancellation, remaining fetch attempts and the holding's elapsed-time budget before issuance, and its wait is cancellation-aware.
A valid `Retry-After` (seconds or HTTP date) is a minimum wait: a longer delay is honored only if the holding budget allows it, otherwise the retry is omitted and URL suppression retains the server's minimum; an overflowing seconds value never becomes an immediate retry.
Missing or malformed headers use the default delay; the cooldown is at least its class's normal duration.
Reqwest's configurable internal retry policy is disabled for this fetcher so the app owns the retry count.
An attempt means an application-managed page fetch, including its existing bounded redirect chain, not a count of TCP connections.

Progress details distinguish document-cache service, remembered URL failures, host cooldowns and live failures; an automatic retry has its own request row.
Failure details include the context and underlying error source chain, retained when a remembered URL failure or host cooldown is reported, so a transport failure names the available cause rather than only the outer fetching context.
Cause text is diagnostic; classification and retry eligibility still use typed evidence, never guesses from that text.
Only actual admitted attempts contribute source telemetry, each once; a memory hit or suppressed destination adds no failed/denied sample, and a recovered retry adds the successful extraction sample beside the original failure.
An unavailable page still contributes the existing research-degradation gap and cannot become citation evidence.
Exhaustion still ends further tool execution, even when a later request might otherwise have been served without spending an attempt.

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
  Every page the model reads carries its **source URL and retrieval timestamp** in the prompt, and the holding's audit record persists the roster of pages shown to the model — address, title, publication date, retrieval time and source tier, never the page text — so a verdict or opportunity can be traced to what the model had in front of it and when; the write-up names the source of each dated fact, a page or the fetched values, as an instruction the app does not check (see [portfolio-analysis.md](portfolio-analysis.md), [storage.md](storage.md)).

## Source quality and evidence weighting

The web loop reaches an unbounded set of domains, so the suite weighs *where evidence comes from*, not only *what it says*.
This is the **engine-computes / model-interprets** spine applied to sourcing: the **app computes objective source signals** and the **model interprets content with those signals in view**, against the per-domain metadata in [data-sources.md §Source registry and evidence tiers](data-sources.md#source-registry-and-evidence-tiers).

**The load-bearing rule: source quality informs, it never gates.**
A low-tier or thinly-sourced finding **lowers conviction**; it does **not** remove a candidate or claim from consideration.
This governs the **evidence tiers (0–5)** — a low tier weights down, never excludes; the **one exception is the explicit `deny` policy**, which drops junk that isn't evidence at all (SEO mills, AI-generated quote pages, PR-spam) at search-filter and fetch-gate ([data-sources.md §Source registry and evidence tiers](data-sources.md#source-registry-and-evidence-tiers)) — excluding a non-source isn't gating *on quality*, it is keeping spam out of the evidence base (**tiers grade, `deny` excludes, nothing between is gated**).
The rule's scope is **source quality** — the tier gradient and its soft annotations (`recencyScore` against `freshnessSlaDays` stays a weight, never a gate).
It does **not** govern *claim freshness*: whether a floor-bearing input (a leading metric, statements) is current enough to gate on is a **job-owned evidence-floor question**, decided by Trade Opportunities' typed freshness basis ([trade-opportunities.md §Starting parameters](trade-opportunities.md#starting-parameters-calibratable)) — the deny-style carve-out for age, deliberately narrow so `freshnessSlaDays` can never silently turn from a weight into a gate.
This is deliberate and protects the job's edge — Trade Opportunities' whole value is surfacing **under-covered, early-inflection names no structured feed carries** ([trade-opportunities.md §The pipeline](trade-opportunities.md#the-pipeline)), and those names live in Tier-3/4 coverage by definition; gating discovery on source tier would systematically suppress exactly what the job hunts.
The discipline mirrors the suite's existing stance — positioning held out of the grade until calibrated, the since-flagged read kept cap-only — quality is a **conviction input, never a survival gate**.

**The evidence annotation, split by who can know it.**
Each fetched document is annotated, and the split is strict:
- **App-computed (deterministic):** `sourceTier` (from the registry / default heuristic), `extractionQuality` (0–1, how much real article body the readability pass recovered vs a thin paywall / JS stub), `recencyScore` (0–1, against the source's `freshnessSlaDays`), `primarySourceBonus`, and a paywall / JS-stub flag.
  Portfolio's research prompts render the tier, the evidence kinds, the extraction quality and the stub flag beside the page's publication and retrieval dates; the recency score is computed and persisted but not rendered, since the dates say more.
- **Model-derived (judgment):** how specific a page's statements are, whether it contradicts another page, and which of the write-up's statements rest on it — these are reasoning, carried in the model's write-up, and never dressed up as app-computed.

The model sees the app's source-quality judgment **alongside** the text and weighs evidence accordingly — a Tier-0 filing outweighs a Tier-4 blog on a reported number, while a Tier-3 specialist outweighs a Tier-2 generalist on its own vertical (the `evidenceKinds` match).

**Lane policy.**
The same source is treated differently by lane:
- **Discovery** — **soft preference**, never a hard floor: breadth is the point, so a promising lead from a low-tier source is still pursued, only weighted down.
- **Per-candidate / per-holding validation** — **stricter weighting**: a name's verdict should rest on higher-tier corroboration, and a statement resting only on Tier-4/5 sources is written as low-confidence (still surfaced — informing, not gating).
- **Portfolio** leans to primary filings, company IR, transcripts, and material-event reporting; **Trade Opportunities** leans to specialist and value-chain sources.
  These are the per-route source strategies of [trade-opportunities-workflow.md §Step 3b](trade-opportunities-workflow.md#step-3b-model-led-hypothesis-research), now expressed through the registry's `evidenceKinds`.

**Source-diversity / syndication caps.**
Five outlets reprinting one Reuters wire are **one** independent source, not five.
The loop collapses near-duplicate and same-canonical-origin hits so apparent corroboration can't be inflated by syndication — independence is counted by *origin*, not by *URL count*.

**A disconfirming-fetch pass.**
Beyond the existing bear case and adversarial passes ([trade-opportunities.md §The research method](trade-opportunities.md#the-research-method)), once the topics are written up the loop spends one bounded pass searching specifically for **what would disprove them** — a disconfirming *fetch*, not just a disconfirming *prompt*.
It is **spent from the existing per-item / per-route fetch + wall-clock budget, never added on top of the ceilings** ([§The research loop and context management](#the-research-loop-and-context-management)): a high-priority item *within* that budget that **fail-softs to a recorded gap (and lower conviction) when the budget is already exhausted**, so a thesis is tested against contrary evidence before it earns conviction without ever breaching the loop's hard bound.
Portfolio's placement of the pass — per holding, after its topics — is specified at [portfolio-workflow.md §Step 6c](portfolio-workflow.md#step-6c-bounded-web-research).
Trade Opportunities' placement — per candidate, after its Step-5d topics — is specified at [trade-opportunities-workflow.md §Step 5d](trade-opportunities-workflow.md#step-5d-bounded-web-research).

**Extraction telemetry.**
The fetch layer tracks, per domain, how often it recovers full article text vs a thin paywall / JS stub (the same telemetry stance as the report's document-truncation tracking).
This feeds two things: a domain's `extractionProfile` in the registry, and the **health test** that decides whether a connected subscription is actually yielding value (below) — so a source that renders poorly through a non-browser fetch is never silently trusted as if it did.
It also counts, per domain, every live attempt that failed past the app's own guard — a transport error, an unresolvable host, an HTTP error status, a content or redirect bound — and, within that, the attempts the source answered HTTP 401 or 403, the paywall / bot-block read.
A policy refusal (scheme, deny list, a non-public address) never reached the source and is not its record.
The failed and denied counts sit beside the full / thin pair and never enter the profile or render-first derivation, which reads served pages only: a refused fetch says nothing about how the domain's pages extract.

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

Authenticated fetch holds the same safety posture as the rest of the loop ([§Safety and provenance](#safety-and-provenance)): SSRF guards still apply, fetched content is still data-not-instructions, and every page read still enters the roster with its source URL + timestamp.
The one honest caveat is operational and outside the app's control — automated access to a subscription can run against a publisher's terms and sessions can be invalidated server-side, so Connected Sources is **best-effort enrichment**, never a guaranteed source.

## Failure posture

Web research is **fail-soft**.
A failed search, a timed-out fetch, or an empty result degrades the evidence for the item under study; it does not fail the run.
The model proceeds with whatever evidence landed, and the thinner evidence is reflected in the analysis (for example, lower conviction), consistent with the suite's honest-degradation stance (see [local-models.md §Failure posture](local-models.md#failure-posture)).
