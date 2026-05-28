# Questions

*Open and deferred items, one `##` heading per item. Resolved items move to `RESOLVED.md`.*

## Q1: How do the 16 analyst skills plug into the pipeline
Status: open
Added: 2026-05-28

docs/analyst-skills.md: "The following reusable skills are included in MVP. These skills operate as structured reusable prompts with expected output schemas." Sixteen skills are then defined (Market Regime Analysis, Narrative vs Reality, etc.).

docs/agents.md and docs/weekly-report-workflow.md describe the Main Agent, the three analyst agents, and all 17 pipeline steps without ever invoking, assigning, or referencing these skills.

The skills are declared as MVP scope but no doc says who runs them (Main Agent? analyst agents? application layer?), when in the 17-step flow they fire, whether they are mandatory or selected per report, or where the "expected output schemas" are defined. A resolution must specify how the skill library is wired into the agent/pipeline behavior.

## Q2: Mapping multi-axis regime analysis onto the single fixed market_regime label
Status: open
Added: 2026-05-28

docs/storage.md §SQLite: `market_regime` is a single label from a fixed vocabulary — `risk-on`, `risk-off`, `mixed`, `late-cycle`, `recessionary`, `recovery` — and "The main agent selects the label that best fits the current regime."

docs/analyst-skills.md §Market Regime Analysis: the skill assesses the regime along several independent axes — risk-on/risk-off, liquidity- vs earnings-driven, inflation- vs growth-sensitive, broadening vs narrowing leadership.

The fixed vocabulary mixes a risk-posture axis (`risk-on`/`risk-off`/`mixed`) with a cycle-stage axis (`late-cycle`/`recessionary`/`recovery`), so a single market can match more than one label (e.g. both `risk-on` and `late-cycle`), and the multi-axis skill output has no defined reduction to one label. A resolution must specify how the regime assessment collapses to one stored value (or whether the schema should carry more than one regime dimension).

## Q3: Which external data provider credentials gate execution
Status: open
Added: 2026-05-28

docs/configuration.md §External Data Provider Credentials: collects credentials for Financial Modeling Prep and Tavily; "OpenBB uses configured provider credentials where required by the selected data source"; "FRED, BLS, and GDELT may be accessed through their publicly available APIs when supported." Missing "a required external provider credential" blocks jobs.

docs/data-sources.md: Tavily is "the primary research and news-ingestion system" and GDELT is used for geopolitical coverage (both consumed by the mandatory Step 7); Financial Modeling Prep is used "when direct access is simpler, more complete, or required by the workflow" (supplemental).

Which providers count as "required" for the gate is unspecified: Tavily appears mandatory (primary, drives a non-optional step), FMP reads as supplemental, and OpenBB/FRED/BLS/GDELT may run on public access. A resolution must enumerate the credential set that blocks execution (feeds the gate reconciliation in CONTRADICTIONS.md C3).

## Q4: Where missing-external-credential warnings surface in the UI
Status: open
Added: 2026-05-28

docs/interface.md §Persistent Warning Area: enumerates exactly four warning categories — Missing agent configuration, Missing API tokens, Failed jobs, Missed scheduled jobs — each holding at most one unresolved warning.

docs/configuration.md §External Data Provider Credentials: a missing required external-provider credential "displays a validation warning."

The Persistent Warning Area's fixed category list has no slot for external-data-provider-credential problems. A resolution must say where that warning lives — folded into "Missing API tokens," a new category, a Settings-only inline validation, or elsewhere. (Relatedly, scheduling.md surfaces "skipped job events" in Job Status Visibility, which is also not one of the four Persistent Warning Area categories.)

## Q5: Behavior when a research-inbox document fails to parse
Status: open
Added: 2026-05-28

docs/research-documents.md §Processing at Job Start: "If documents exist, the application parses them and prepares them as professional research sources... After successful processing, the application automatically moves the documents into /research-archive." docs/research-documents.md §User Permissions: the user "cannot manually archive documents."

Only the success path is specified. If a supported-format file is malformed or cannot be parsed, the docs do not say whether it stays in the inbox, is skipped, blocks/fails the job (cf. scheduling.md failure causes), or is surfaced as a warning. Because the user cannot archive manually, an unparseable file would persist in the inbox and be re-attempted every run unless a rule is defined. A resolution must specify the parse-failure handling.

## Q6: Analyst-agent execution order and overall job time budget
Status: open
Added: 2026-05-28

docs/weekly-report-workflow.md §Steps 11–14: Step 11 runs the three analyst agents; Steps 12–14 list Bull, then Bear, then Balanced reviews against the same packet. docs/agents.md §Analyst Agents: each receives the same packet independently and the Main Agent "does not engage in recursive conversations" with them. docs/weekly-report-workflow.md §Step 9 sets limits only for the research phase (50 requests / 30 minutes / depth 2).

It is unspecified whether the three analysts run sequentially or concurrently, and there is no stated overall job time budget covering the analyst and synthesis phases (which may use heavy configurable models). A resolution would clarify execution concurrency and whether any job-level timeout exists beyond the research-phase limit.
