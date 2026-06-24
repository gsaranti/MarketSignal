# Web Research Tool

The local analysis suite reaches the open web through one tool — **search, fetch, and extract** — that the Rust orchestrator runs on the model's behalf. A stage requests a search or a page; the application layer performs the network I/O and returns clean text. The model never touches the network, holding the same pure-stage boundary as the report pipeline (see [local-models.md](local-models.md), [agents.md](agents.md)). The tool is **keyless, local-first, and cost-free** by default.

## The tool-call loop

A research stage runs as a bounded loop: the model emits a `web_search` (or `web_fetch`) tool call, the orchestrator executes it, and the result is returned as a tool message for the next turn. The orchestrator — not the model — owns every request, so the loop is bounded the way the report's research executor is: a cap on requests and on wall-clock time per item, polled at each request boundary (see [report-workflow.md](report-workflow.md)). The model decides *what* to look up; the application decides *how much* it is allowed to.

## Search backend: SearXNG

Search is served by a **self-hosted SearXNG** instance running locally and queried over its JSON API on the loopback interface. SearXNG is a metasearch front end: it fans a query out to real engines, parses and merges the results, and returns structured hits (title, URL, snippet) the orchestrator can rank and fetch.

**Rationale (SearXNG over a paid search API):**
- **cost-free, no per-query credit ceiling** — a deep multi-step research loop over many items can't exhaust a metered quota (though upstream engines can still rate-limit or CAPTCHA individual queries — see Failure posture);
- **local-first** — no API key and no paid-service dependency in the default path (the local instance still queries public engines, but you don't rely on any single provider's API);
- **engine diversity** — results aren't bound to one engine's ranking or rate limits.

Two configuration facts are load-bearing, and the app sets them up. SearXNG's **JSON format is disabled by default** (an unset format returns HTTP 403), so JSON output is enabled; and the **bot limiter is disabled** for the single-user loopback instance (it exists to protect a public instance from bots, which a private local one doesn't need). The instance runs general engines that don't aggressively CAPTCHA automated queries (e.g. Brave, Mojeek, Qwant, DuckDuckGo); Google is deprioritized because it CAPTCHA-walls hardest. The app health-checks the instance the same way it supervises the model daemon (see [local-models.md §Serving runtime](local-models.md#serving-runtime)).

## Fetch and extraction

Search returns links; the tool then **fetches the top results and extracts readable text**. The fetch is a plain HTTP GET with a normal User-Agent and a timeout; extraction strips navigation, ads, and boilerplate down to the article body so the model reasons over content, not page chrome. Readability extraction is done in Rust (a `readability.js`-style article extractor). Pages that are paywalled or render their content with client-side JavaScript return thin text to a non-browser fetch — a fetch-layer limit, not an extractor failure — and such results simply contribute less evidence rather than breaking the loop.

## Tavily fallback

When the local SearXNG instance is unreachable or returns nothing (for example, the user hasn't started it), the tool **falls back to Tavily**, the LLM-optimized search API the app already integrates for the report's research ([data-sources.md](data-sources.md)). Tavily's free tier is metered, so it is the degraded path, not the default — it keeps a local job useful when the local search backend is down rather than serving as the primary source.

## Safety and provenance

Because the model chooses what to fetch, fetching is treated as an untrusted operation:

- **SSRF protection.** Fetches are restricted to `http`/`https` and to public hosts — private, loopback, and link-local address ranges are blocked (this matters specifically because the app's own Ollama and SearXNG run on loopback), redirects are capped and re-validated against the same rules, and responses are bounded by size and content type (HTML/text only).
- **Untrusted content.** Fetched page text is data, not instructions: it is inserted into the prompt as quoted evidence and never interpreted as a directive, so a page carrying injected instructions cannot redirect the analysis.
- **Provenance.** Every research finding carries its **source URL and retrieval timestamp**, so a verdict or opportunity can be traced to what it was based on and when — feeding the run's audit record (see [portfolio-analysis.md](portfolio-analysis.md), [storage.md](storage.md)).

## Failure posture

Web research is **fail-soft**. A failed search, a timed-out fetch, or an empty result degrades the evidence for the item under study; it does not fail the run. The model proceeds with whatever evidence landed, and the thinner evidence is reflected in the analysis (for example, lower conviction), consistent with the suite's honest-degradation stance (see [local-models.md §Failure posture](local-models.md#failure-posture)).
