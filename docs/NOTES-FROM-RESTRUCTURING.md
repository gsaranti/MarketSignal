# Notes from Restructuring

Items surfaced during the restructuring pass. Captured for human review; no resolutions proposed and not exhaustive.

Line references point into the original [requirements.md](../requirements.md).

## Apparent Contradictions

- **Settings inventory excludes external provider credentials.** `## Settings` (lines 59–65) and the Settings tree in `### Main Layout` (lines 50–54) enumerate four Settings items, none of which mention external data provider credentials. But `### External Data Provider Credentials` (lines 522–537) states "The Settings section includes credential configuration for: Financial Modeling Prep, Tavily." The upstream inventories and this subsection disagree on what Settings contains.

- **"By default, all are enabled" with only one job.** `### Job Controls` (lines 154–159) lists a single job type (Enable/Disable Weekly Market Job) and then states "By default, all are enabled." The plural "all" implies more than one job, but only one is specified anywhere in the source.

## Terms Used Inconsistently or in Two Senses

- **"skipped" and "missed" overlap.** `### System Sleep Behavior` (lines 119–122) says a sleep-time job "is skipped" and the application "does not attempt to retroactively execute the missed job" — using both terms for the same event. `### Missed Job Detection` (lines 186–203) defines a missed job as one where "the machine was asleep" is among the causes. The relationship to concurrent-protection "skipped" executions (line 143–146) is left implicit.

- **"Settings" used for both the UI panel and the underlying config state.** The term denotes the UI section in `### Main Layout` (line 50) and `## Settings` (line 59), but also the runtime configuration data in phrases like "loading application settings" (line 588) and "settings saving is disabled" (line 507).

- **Warning de-duplication scope is unclear.** `### Error Handling` (lines 183–184) states that additional failing jobs do not create duplicate unresolved warnings, but the source does not say whether the same de-duplication rule applies to the other Persistent Warning Area categories (missing agent configuration, missing API tokens, missed scheduled jobs).

## Apparent Gaps a Builder Would Need

- **Embedding model for LanceDB is unspecified.** `### Step 4: Retrieve Relevant Vector Memory` (lines 638–650) and `### LanceDB Vector Memory` (lines 1402–1416) describe what is retrieved and stored, but the source does not say which model generates the embeddings or how text is chunked before embedding.

- **"Workflow limits" are referenced but undefined.** `### Step 9: Perform Dynamic and Forward-Looking Research` (line 747) says the application "applies workflow limits" without specifying concrete bounds (request count, depth, duration, token budget).

- **The source of the initial ~500 headlines is unspecified.** `### Step 7: Gather and Filter News` (lines 711–719) describes the funnel from ~500 to ~5, but the source does not say which feeds produce the initial set. Tavily and GDELT are both available per `## Data Sources`, but their roles in seeding this funnel are not defined.

- **"Market regime label" has no defined vocabulary or schema.** It appears as report metadata in `### Step 2` (line 607) and as a stored field in `### SQLite` (line 1389), but the set of valid labels (or whether it is free-form) is not specified.

- **"Structured report summary metadata" has no schema.** Referenced in `### Step 2` (line 605) and `### SQLite` (line 1388), and a "report summary" is also written to LanceDB (lines 933, 1403), but no field-level schema is provided.

- **HTML-from-Markdown conversion is unspecified.** `### Step 17: Generate HTML and Update UI` (lines 943–955) says HTML is generated from Markdown for in-app rendering, styling, charts, and PDF generation, but the source does not name a renderer, styling source, or how charts/graphs/tables in the HTML are produced from underlying data.

- **Persistent Warning Area dismissal behavior is ambiguous.** `### Missed Job Detection` (line 199–202) says the user may "dismiss the warning" but the source does not state whether dismissal removes the warning permanently or only until the next occurrence.

- **Stored report filenames on disk are not defined.** `### Step 16` (line 928–934) says the Markdown report is saved "to persistent local storage" and `### Export Naming` (lines 1447–1454) defines an export-filename convention, but the on-disk name of the stored canonical Markdown report is not specified.

## Duplicated Content Consolidated

- **Analyst-agent responsibilities and posture** (Bull / Bear / Balanced) appeared in both `## Agent Pipeline Architecture` (lines 377–424) and Steps 12–14 (lines 846–889). Consolidated into [agents.md §Bull Analyst / §Bear Analyst / §Balanced Analyst](agents.md#bull-analyst); workflow Steps 12–14 are now stubs that reference back.

- **"Analyst agents are not optional tools / not forced into disagreement / are valid to agree or disagree" prose** appeared in both `## Agent Pipeline Architecture` (lines 383–421) and `### Step 11: Run Analyst Agents` (lines 824–844). Consolidated into [agents.md §Analyst Agents](agents.md#analyst-agents).

- **The Markdown-canonical / HTML-for-presentation rule** ("Agents never ingest or reason over HTML reports") appeared in `### Step 2` (line 604), `### Step 17` (line 953), `## Report Structure` (lines 1224–1240), and the opening of `## Export System` (line 1421). Canonical home is now [report-structure.md](report-structure.md); the other files reference it.

- **The "scheduled jobs enabled by default, gated by configuration completeness" rule** appeared in `### Job Controls` (lines 161–164) and is paralleled by gating language in `## Model and API Configuration` (lines 515–521) and `### External Data Provider Credentials` (lines 533–537). The execution gate now lives in [configuration.md](configuration.md); [scheduling.md §Job Controls](scheduling.md#job-controls) references it.

## External References Without Canonical Links

The source links the data and research APIs to their docs (OpenBB, FMP, FRED, BLS, Tavily, GDELT) but leaves several other external references unlinked, where a builder would benefit from authoritative documentation.

- **Technology stack.** `## Overview` (lines 4–8) names Tauri, Vue, SQLite, and LanceDB without links to their project documentation.

- **LLM provider APIs and model identifiers.** OpenAI and Anthropic are referenced as providers (lines 302–303, 487–493) and named models — GPT-5, GPT-5 mini, Claude Opus, Claude Sonnet, Claude Haiku (lines 432, 444, 464, 487–493) — without links to provider API documentation or model-specific reference pages. The fixed internal model references in `## Fixed Internal Model Usage` (lines 427–476) are similarly unlinked.

- **PDF renderer.** `### PDF Export` (line 1444) says source links are preserved "when supported by the PDF renderer" without naming the library, tool, or rendering pipeline that performs PDF generation.
