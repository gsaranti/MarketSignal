# Notes from Restructuring

Records the content consolidations applied during the restructuring of [requirements.md](../requirements.md) into this `docs/` directory. The consolidations themselves are reflected in the docs with cross-references; this file documents what was merged and from where.

Line references point into the original [requirements.md](../requirements.md).

## Duplicated Content Consolidated

- **Analyst-agent responsibilities and posture** (Bull / Bear / Balanced) appeared in both `## Agent Pipeline Architecture` (lines 377–424) and Steps 12–14 (lines 846–889). Consolidated into [agents.md §Bull Analyst / §Bear Analyst / §Balanced Analyst](agents.md#bull-analyst); workflow Steps 12–14 are now stubs that reference back.

- **"Analyst agents are not optional tools / not forced into disagreement / are valid to agree or disagree" prose** appeared in both `## Agent Pipeline Architecture` (lines 383–421) and `### Step 11: Run Analyst Agents` (lines 824–844). Consolidated into [agents.md §Analyst Agents](agents.md#analyst-agents).

- **The Markdown-canonical / HTML-for-presentation rule** ("Agents never ingest or reason over HTML reports") appeared in `### Step 2` (line 604), `### Step 17` (line 953), `## Report Structure` (lines 1224–1240), and the opening of `## Export System` (line 1421). Canonical home is now [report-structure.md](report-structure.md); the other files reference it.

- **The "scheduled jobs enabled by default, gated by configuration completeness" rule** appeared in `### Job Controls` (lines 161–164) and is paralleled by gating language in `## Model and API Configuration` (lines 515–521) and `### External Data Provider Credentials` (lines 533–537). The execution gate now lives in [configuration.md](configuration.md); [scheduling.md §Job Controls](scheduling.md#job-controls) references it.
