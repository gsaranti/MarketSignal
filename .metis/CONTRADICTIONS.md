# Contradictions

*Open and deferred items, one `##` heading per item. Resolved items move to `RESOLVED.md`.*

## C1: Who authors the research plan — Main Agent or the fixed routing model
Status: open
Added: 2026-05-28

docs/agents.md §Main Agent: the Main Agent is responsible for "creating structured research requests" and "dynamically guiding research priorities"; and (intro) "When deeper research is needed, the main agent creates structured research requests that the application layer executes against configured data sources."

docs/agents.md §Research Routing (Fixed Internal Models): a non-configurable Claude Sonnet model handles "determining which topics deserve deeper analysis," "prioritizing research depth," and "deciding which themes/subsectors/geopolitical events matter most."

docs/weekly-report-workflow.md §Step 8: "The routing model produces a structured research plan, and the application layer is responsible for executing that plan." The Main Agent does not appear between Step 4 and Step 10 (where it builds the packet from already-curated evidence); Step 9 executes the "approved research plan" without naming an approver.

The same responsibility — producing the structured research plan/requests that the application layer executes — is assigned both to the user-configurable Main Agent and to the fixed routing model. A resolution must name which one owns research planning (and what "dynamically guiding research priorities" means if the Main Agent does not produce the plan), and who "approves" the plan in Step 9.

## C2: API-token validation is conditional, but fixed internal models always require both providers
Status: open
Added: 2026-05-28

docs/configuration.md §API Tokens: "The user must provide: OpenAI API token, Anthropic API token." Then, in the same section: "When a user selects a provider for an agent, the corresponding API token is required" — with examples that gate each token on the user's agent model selection ("selecting an OpenAI model requires a valid OpenAI API token").

docs/agents.md §Fixed Internal Models: headline filtering and data extraction always use OpenAI GPT-5 mini; research routing always uses Anthropic Claude Sonnet. docs/storage.md §Embeddings: embeddings always use OpenAI text-embedding-3-large "using the configured OpenAI API token."

The token rule reads two ways at once (both tokens mandatory vs. each token required only when its provider is selected), and neither reading accounts for the fixed internal stages: because filtering/extraction/embeddings always need OpenAI and routing always needs Anthropic, both tokens are in fact required for any job regardless of which providers the user picks for the four agents. A user could select all-Anthropic agents, satisfy the conditional check, and still be unable to run. A resolution must state whether both LLM tokens are unconditionally required.

## C3: Step 1 validation list omits the external-data-provider-credential gate
Status: open
Added: 2026-05-28

docs/weekly-report-workflow.md §Step 1: enumerates the pre-run checks — job enabled, no other job running, "the Main Agent and all Analyst Agents are configured," "required API tokens exist for selected model providers," and network access — and points to canonical homes for "each check." External data provider credentials are not in the list.

docs/configuration.md §External Data Provider Credentials: "If a required external provider credential is missing: dependent jobs do not execute, manual report execution is disabled, the application displays a validation warning."

Configuration.md makes missing external-provider credentials an execution gate, but the workflow's Step 1 validation checklist neither lists that check nor points to its canonical home. A resolution must reconcile the gate set so the validation step accounts for external-provider credentials (see also QUESTIONS.md Q3 on which credentials are required).

## C4: Deep-dive topic selection is attributed to two different stages/models
Status: open
Added: 2026-05-28

docs/weekly-report-workflow.md §Step 7 (Gather and Filter News): the news funnel runs entirely under the fixed low-cost headline-filtering model and ends at "~10 important stories → ~5 deeply analyzed topics."

docs/weekly-report-workflow.md §Step 8 (Perform Research Routing): "Research routing determines which topics deserve deeper analysis for the current report," performed by the fixed mid-tier routing model (Claude Sonnet, per docs/agents.md §Research Routing).

Both steps describe selecting the set of topics that receive deeper analysis, assigning that selection to two different fixed models (GPT-5 mini headline filtering in Step 7, Claude Sonnet routing in Step 8). A resolution must clarify where the headline funnel ends and routing begins — e.g., whether Step 7's "~5 deeply analyzed topics" is the routing input, the routing output, or a separate count.
