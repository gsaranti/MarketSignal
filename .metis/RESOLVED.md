# Resolved

*Thin archive trail of resolved items. The substance lives in the updated source docs.*

## C1: Who authors the research plan — Main Agent or the fixed routing model
Resolved: 2026-05-28
Summary: Fixed Claude Sonnet routing model owns the executable research plan (Step 8); the main agent shapes research only indirectly via routing inputs and builds the condensed packet at Step 10. Corrected agents.md's authorship claims.

## C2: API-token validation conditional vs. fixed models always needing both providers
Resolved: 2026-05-28
Summary: Both OpenAI and Anthropic tokens are unconditionally required, since fixed internal stages span both providers. Rewrote configuration.md §API Tokens and fixed the Step 1 token check.

## C3: Step 1 validation omits the external-data-provider-credential gate
Resolved: 2026-05-28
Summary: Added an external-credential check + canonical-home pointer to weekly-report-workflow.md §Step 1, matching configuration.md's existing gate. Which credentials are "required" left to Q3.

## C4: Deep-dive topic selection attributed to two stages/models
Resolved: 2026-05-28
Summary: Step 7 funnel ends at ~10 clustered important stories (headline filter's output); the ~5 deeply analyzed topics are selected by research routing (Step 8) and analyzed in Step 9. Trimmed the funnel and added a clarifying sentence.

## Q1: How the 16 analyst skills plug into the pipeline
Resolved: 2026-05-28
Summary: Skills are a shared analytical library for the three analyst agents + main agent (synthesis), surfaced via progressive disclosure — agents see each skill's frontmatter, then request the full skill (prompt + output schema) on demand per research packet; only the relevant subset runs. Added a "How Skills Are Used" section to analyst-skills.md.

## Q2: Mapping multi-axis regime analysis onto the single market_regime label
Resolved: 2026-05-28
Summary: Split the single market_regime field into two axes — risk_posture {risk-on, risk-off, mixed} and market_cycle {late-cycle, recessionary, recovery}. Updated storage.md (definition + report-summary schema) and weekly-report-workflow.md Step 2 metadata.

## Q3: Which external data provider credentials gate execution
Resolved: 2026-05-28
Summary: Only the Tavily credential gates execution (primary news/research, mandatory Step 7); FMP is optional/supplemental; OpenBB/FRED/BLS/GDELT are not user-credential-gated. Updated configuration.md §External Data Provider Credentials. *(Refined 2026-06-01 by OA2: after OpenBB was dropped, FMP became the sole source of the non-optional baseline scan and now gates execution too.)*

## Q4: Where missing-external-credential warnings surface in the UI
Resolved: 2026-05-28
Summary: Added a fifth Persistent Warning Area category, "Missing provider credentials" (canonical home: configuration.md §External Data Provider Credentials). Updated interface.md layout tree, surfaces list, and triggers list. Skipped-job events stay in Job Status Visibility by design.

## Q5: Behavior when a research-inbox document fails to parse
Resolved: 2026-05-28
Summary: Fail-soft — an unparseable document is skipped and logged, the job continues, the file stays in /research-inbox (not archived), and it's shown in the Research Documents panel in an error state for the user to fix or delete. Added a "Parse Failures" section to research-documents.md.

## Q6: Analyst-agent execution order and overall job time budget
Resolved: 2026-05-28
Summary: The three analyst agents run concurrently (independent, shared packet; Steps 12–14 numbering is not an order). No overall job timeout beyond the Step 9 research-phase cap — analyst/synthesis are fixed single-pass stages, and stuck calls are handled by Error Handling. Updated weekly-report-workflow.md Steps 9 and 11.

## OA1: OpenBB (primary financial layer) vs. calling provider REST APIs directly
Resolved: 2026-06-01
Summary: Dropped OpenBB from the MVP. OpenBB Platform is Python-only (no Rust SDK), so it would force a ~100–250 MB Python sidecar into the signed/notarized macOS bundle — the project's riskiest packaging surface — while buying little: its value is cross-provider normalization, which is negligible at three providers of disjoint responsibility (FMP markets, FRED macro, BLS labor). The app calls FMP/FRED/BLS REST directly from Rust (`reqwest`/`serde`); FMP is the primary financial-data source, FRED/BLS macro via public APIs. Updated data-sources.md (FMP primary; OpenBB noted as evaluated-not-adopted) and .metis/BUILD.md (resolved the open bet).

## OA2: Does the FMP credential gate execution
Resolved: 2026-06-01
Summary: Yes — refines walk Q3. With OpenBB gone, FMP is the sole source of the non-optional Step 6 baseline market-data scan, so a missing FMP credential now blocks a run alongside Tavily (FMP was previously optional/supplemental). Updated configuration.md §External Data Provider Credentials.
