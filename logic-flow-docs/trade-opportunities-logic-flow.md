# Trade Opportunities: logic flow

> This describes the designed job behavior.  
> Some parts are not implemented yet.

`Gate → Load context → Discover names → Narrow list → Deep-check each name → Build matrix → Maintain old ideas → Save → Display`

## Important terms

- **DTO — Discover Trade Opportunities**
  - Finds new ideas.
  - Maintains existing ideas.
  - Runs the full workflow.

- **ATO — Audit Trade Opportunities**
  - Rechecks opportunities you select.
  - Has Quick and Deep modes.
  - Does not discover new names.

- **Candidate**
  - A company being investigated.
  - Not yet approved as an opportunity.

- **Opportunity**
  - A candidate that passed all required checks.

- **Hypothesis**
  - A testable investment idea.
  - Example: “AI data-center growth will benefit cooling suppliers.”

- **Route**
  - A research direction.
  - Examples: supply chain, regulation, customer spending, technical bottlenecks.

- **Coverage debt**
  - A route, industry, or active theme has not been researched recently.
  - Causes the app to reserve a research-route slot.

- **Limited-history evidence**
  - Older evidence for a new listing, spin-off, or changed business perimeter.
  - Must map cleanly to the current company.

- **Research-watchlist refresh**
  - Small current-search check on one stored research-only metric.
  - Does not decide whether the company is investable.

- **Leading metric**
  - A number expected to move before profits or the stock price.
  - Examples: backlog, bookings, subscriber additions, estimate revisions.

- **Catalyst**
  - An event that may make the market notice the opportunity.
  - Example: earnings, product launch, contract, regulatory decision.

- **Thesis milestone**
  - One step in the expected path from today to the thesis paying off.
  - May include an evidence-backed estimated date range.

- **Falsifier**
  - A measurable condition that would show the thesis is wrong.
  - Example: backlog declines for two reporting periods.

- **Archetype**
  - The type of business or opportunity.
  - Determines which financial signals matter most.

- **Conviction**
  - Confidence in the investment thesis.
  - Separate from risk.

- **Risk tier**
  - How risky the company appears.
  - High, Medium, or Low.

- **Horizon**
  - When the thesis is expected to pay.
  - Short, Mid, or Long.

- **Gate**
  - A mandatory rule.
  - Failure prevents a new candidate from entering the matrix.

- **Opportunity graph**
  - The job’s discovery memory.
  - Stores hypotheses, watchlist names, and their relationships.

- **Episode**
  - A dated record of a decision.
  - Used later to measure whether the decision worked.
  - A picked episode records an accepted opportunity.
  - A shadow episode records a rejected or deferred candidate.

- **Shadow ledger**
  - Stores candidates the job considered but did not select.
  - Used to detect missed winners.

- **Cheap re-derivation**
  - Fast, model-free refresh.
  - Recalculates numbers.
  - Can raise a warning.
  - Cannot remove an opportunity.

- **Deep re-evaluation**
  - Full financial and web-research pass.
  - Uses the reasoning model.
  - The only process allowed to archive an opportunity.

## Main data sources

- **FMP**
  - Company profiles.
  - Financial statements.
  - Estimates and revisions.
  - Earnings history.
  - Insider and congressional activity.
  - News, events, peers, and live quotes.

- **FRED**
  - Treasury rates.
  - Economic data.
  - Economic-release calendar.

- **Stooq**
  - Historical stock, sector, market, and commodity prices.

- **SEC EDGAR**
  - Official company filings.
  - Restatements and auditor changes.

- **FINRA**
  - Short-interest data.

- **CFTC**
  - Commodity-futures positioning.

- **CBOE**
  - Broad options-market sentiment.

- **Charles Schwab**
  - Per-company option chains.
  - Current holdings for the final owned/not-owned label.

- **SearXNG**
  - Web search for discovery and company research.

- **Tavily**
  - Backup search for per-company research only.
  - Not used for discovery.

- **Local storage**
  - Previous matrix.
  - Opportunity graph.
  - Prior decisions and outcomes.
  - Market Signal house view.

---

# DTO: Discover Trade Opportunities

## Step 1 — Start and safety checks

- **Data retrieved**
  - No investment data yet.

- **Checks**
  - No other Market Signal job is running.
  - Local reasoning model is available.
  - Embedding model is available.
  - Schwab connection is configured.
  - FMP and FRED credentials exist.

- **Model**
  - None.

- **Output**
  - Job starts.
  - Or the app explains what is missing.

---

## Step 2 — Load shared market context

- **Data retrieved**
  - Latest Market Signal house view.
  - Fixed investor-profile preset.
  - Previous opportunity matrix.
  - Previous opportunity graph.
  - Discovery coverage ledger.
  - `DGS2` and `DGS10` Treasury rates from FRED.
  - Historical `DGS10` data for valuation calculations.
  - Commodity prices from FRED and Stooq.
  - Commodity positioning from CFTC.
  - Broad put/call data from CBOE.
  - Economic-release dates from FRED.

- **Logic**
  - Ignore the house view if older than one week.
  - Use `DGS10` in price-target calculations.
  - Use `DGS2` in minimum-return requirements.
  - Load previous ideas for continuity.
  - Treat all rates and returns as decimal values internally.

- **Model**
  - None.

- **Output**
  - One shared context packet.
  - Reused for every candidate.

---

# Step 3 — Discover candidates

Three discovery feeders run.

## Step 3a — Structured market screens

- **Data retrieved**
  - FMP company screener.
  - FMP insider-buy feed.
  - FMP earnings and event calendars.
  - FMP merger, IPO, filing, and market-mover feeds.
  - FINRA short-interest file.
  - Commodity prices already loaded in Step 2.

- **Logic**
  - Keep active US-listed equities.
  - Apply minimum price, volume, and market-cap rules.
  - Tag companies by size, sector, and industry.
  - Find:
    - Insider-buy clusters.
    - Short-interest extremes.
    - Recent positive earnings surprises.
    - New corporate events.
    - Commodity-price turns.
  - Standardize earnings surprises against the company’s history.
  - Do not perform full financial scoring yet.

- **Model**
  - None.

- **Output**
  - Broad candidate list.
  - Each candidate carries its discovery signal.

---

## Step 3b — Model-led hypothesis discovery

- **Data retrieved**
  - House view.
  - Previous opportunity graph.
  - FMP news and articles.
  - Economic-release schedule.
  - Current web sources through SearXNG.

- **Model — planning call**
  - Chooses a limited set of research routes.
  - Examples:
    - Supply-chain changes.
    - Regulation.
    - Technical bottlenecks.
    - Customer spending.
    - Industry history.
    - Major technology events.
  - One mandatory route ignores previous ideas.
  - Purpose: prevent the job from becoming anchored.

- **Coverage rotation**
  - App finds the stalest route type and coverage subject.
  - Coverage subjects include broad industries and active themes.
  - Reserves the next route slot after the outside-view route.
  - Uses calendar age, not number of job runs.
  - A completed search counts even when it finds no opportunity.
  - A failed search does not clear the coverage debt.
  - Cannot force a hypothesis or candidate.

- **Research loop**
  - For each route:
    - Split the route into focused topics.
    - Start a clean model conversation for each topic.
    - Search and fetch sources.
    - Allow up to three passes per topic.
    - Stop when the time or fetch budget is reached.

- **Model — hypothesis work**
  - Determines:
    - What is changing.
    - Why it matters economically.
    - Which part of the value chain benefits.
    - Who has pricing power.
    - Which public companies are exposed.
    - Which leading metric would prove the idea.
    - What could invalidate it.
    - Why the idea may already be priced in.

- **App validation**
  - Verify each ticker exists.
  - Verify it is tradable and US-listed.
  - Check the hypothesis score against fixed thresholds.
  - Drop unsupported technology-event claims.
  - Keep weaker but credible ideas on the watchlist.

- **Large-route handling**
  - Split large research routes into smaller pieces.
  - Distill each piece.
  - Combine them into hypothesis cards.

- **Output**
  - Promoted hypothesis cards with candidate names.
  - Watchlist hypotheses.
  - Sources and discovery lineage.

---

## Step 3c — Recheck the old watchlist

- **Data retrieved**
  - Stored watchlist.
  - FMP metrics.
  - Filing data.
  - FINRA short interest when needed.

- **Logic**
  - Recheck each watchlist metric by class:
    - `structured`: every run.
    - `filing`: when a new filing appears.
    - `research`: when discovery finds it again or the targeted refresh lane selects it.
  - Select a small number of `research` metrics for a targeted current-search refresh.
  - Starting cap:
    - One watchlist name per DTO run.
  - Refresh priority:
    - New filing, contract, or material event.
    - Approaching catalyst or thesis milestone.
    - Near-promotion or near-gate candidate.
    - Higher hypothesis score.
    - Oldest successful research refresh.
  - Search only for the stored metric, falsifier, or milestone.
  - Do not rewrite the thesis, targets, conviction, or opportunity record.
  - If the metric improves:
    - Promote the name into the candidate list.
  - If the thesis fails or expires:
    - Retire it.
  - Otherwise:
    - Keep watching it.

- **Capacity logic**
  - Watchlist has a maximum size.
  - Lowest-scoring names leave first.
  - Evicted names still receive shadow episodes.

- **Model**
  - None for structured and filing checks.
  - Targeted reasoning and web research for selected `research` metrics only.

- **Output**
  - Promoted watchlist candidates.
  - Updated watchlist and retired nodes.
  - Targeted-refresh audit record.

---

# Step 4 — Consolidate and allocate research slots

- **Data retrieved**
  - No major new data.

- **Logic**
  - Combine all three discovery feeders.
  - Remove duplicate tickers.
  - Remove funds and non-equities.
  - Recheck basic tradability.
  - Preserve every discovery reason.

- **Research-budget allocation**
  - First: existing opportunities needing maintenance.
  - Second: new candidates.
  - Third: existing opportunities that resurfaced.

- **Maintenance priority**
  - Warning-bearing opportunities.
  - Near-term catalysts.
  - Names close to failing the return gate.
  - Oldest deep research.

- **New-name diversity rules**
  - Protect mid- and small-cap representation.
  - Limit mega-cap concentration.
  - Limit one feeder, archetype, sector, or theme from dominating.

- **Deferred names**
  - Not treated as rejected.
  - Worthy names go to the watchlist.

- **Model**
  - None.

- **Output**
  - Final list receiving expensive Step-5 validation.
  - Record of which existing names receive a deep pass.

---

# Step 5 — Deep validation loop

The following sequence runs once for every selected candidate.

Each candidate is checkpointed separately.

## Step 5a — Classify the archetype

- **Data retrieved**
  - FMP company profile.
  - Income statements.
  - Ratios and key metrics.
  - Segment information.
  - Historical financial patterns.

- **Logic**
  - Calculate classification features:
    - Sector and industry.
    - Margin structure.
    - Recurring revenue.
    - Cyclicality.
    - Discovery signals.

- **Model**
  - Confirms one archetype:
    - Secular compounder.
    - AI infrastructure.
    - Commodity cyclical.
    - Category disruptor.
    - Quality compounder.

- **Validation**
  - Exactly one archetype must result.
  - Failed calls use a deterministic fallback.
  - Existing names cannot change archetype without changed evidence.

- **Output**
  - Authoritative archetype.
  - Confidence and rationale.

---

## Step 5b — Build the candidate dossier

- **Data retrieved**
  - FMP:
    - Statements and ratios.
    - Estimates and revisions.
    - Earnings surprises.
    - Insider and congressional activity.
    - Activist filings.
    - Peers, float, news, and corporate events.
  - SEC EDGAR filings.
  - FINRA short interest.
  - Stooq price history.
  - Live FMP quote.
  - Schwab option chain.
  - Relevant prior analysis from local memory.
  - For an eligible recent listing or separation:
    - S-1 or Form 10 history.
    - Predecessor or carved-out business disclosures.
    - Contracts and dated operating milestones.
    - Customer and supplier evidence.

- **Logic**
  - Cross-check FMP data against SEC filings.
  - Assemble one evidence packet.
  - Keep prior analysis within the same opportunity lifecycle.
  - Give older prior research less influence.
  - App determines limited-history eligibility from listing and corporate-identity facts.
  - Missing provider data cannot create limited-history eligibility.

- **Embedding model**
  - Converts the candidate query into a vector.
  - Retrieves similar prior analysis.
  - Performs no reasoning.

- **Output**
  - Complete candidate dossier.

---

## Step 5c — Calculate the financial picture

- **Data retrieved**
  - Uses the dossier.
  - No model or web research.

- **Calculations**
  - Value, quality, momentum, volatility, and revision composite.
  - Return on invested capital versus capital cost.
  - Owner earnings and reinvestment runway.
  - Leading-metric trend.
  - Standardized earnings surprises.
  - Insider, short-interest, congressional, and options signals.
  - Relative price strength.
  - Liquidity and days-to-cover.
  - Bear, base, and bull price targets.
  - Growth already implied by the current price.
  - Price movement versus actual business improvement.
  - Accounting and governance warnings.
  - Performance since first becoming an opportunity.

- **Forensic events**
  - Restatement: SEC Item 4.02.
  - Auditor change: SEC Item 4.01.
  - Fraud: primary-source research only.
  - Missing evidence becomes `unknown`, not “clear.”

- **Model**
  - None.

- **Output**
  - Deterministic financial analysis.
  - Provisional price targets.
  - Risk and forensic flags.

---

## Step 5d — Research the company

- **Data retrieved**
  - Current web sources.
  - SearXNG first.
  - Tavily if SearXNG fails.
  - Company filings and disclosures where relevant.

- **Research topics**
  - Validate the leading metric.
  - Test theme and economic fit.
  - Assess management and market narrative.
  - Compare with past winners and failures.
  - Seek outside corroboration.
  - Build the mandatory bear case.
  - When limited-history eligible:
    - Confirm source and target company identities.
    - Confirm periods and units.
    - Classify observations as direct, recast, or proxy.

- **Loop**
  - One isolated conversation per topic.
  - Up to three passes per topic.
  - Leading metric and bear case receive priority.
  - Stop at the fetch or time limit.

- **Model**
  - Requests searches and page fetches.
  - Extracts sourced findings.
  - Proposes follow-up questions.

- **Output**
  - Full findings for every topic.
  - Evidence ledger.
  - Mandatory sourced bear case.

---

## Step 5e — Distill the research

- **Data retrieved**
  - No new external data.

- **Model**
  - Condenses the full research.
  - Does not perform new searches.

- **Large-input loop**
  - Normal case:
    - One consolidation call.
  - Large case:
    - Distill each topic separately.
    - Run one final combining call.

- **Model determines**
  - Which findings matter.
  - Whether the research lenses disagree.
  - Severity of contradictions.
  - Key falsifiers.
  - Material forward facts.
  - Possible research-only leading indicators.

- **Typed outputs**
  - Leading-metric observations.
  - Direct forward assumptions.
  - Research target scenarios.
  - Runway evidence.
  - Milestone evidence.
  - Research-only leading indicators.
  - Primary-source forensic events.
  - Sourced bear case.
  - Limited-history evidence when eligible.

- **Output**
  - One structured research object.

---

## Step 5f — Recalculate using validated research

- **Data retrieved**
  - No new data.

- **Logic**
  - Validate all numerical research claims.
  - Reject malformed or unsourced claims.
  - Prefer structured data during unresolved conflicts.
  - Validate any proposed calculation bridge.
  - Recalculate each bridge from sourced facts.
  - Retain the structured-only target for comparison.
  - Add valid new leading-metric observations.
  - Add only direct or explicitly reconciled recast observations to company history.
  - Keep proxy evidence separate from company financial results.
  - Recalculate:
    - Price targets.
    - Leading-metric trend.
    - Business runway.

- **Model**
  - None.

- **Output**
  - Structured-only targets.
  - Research-informed targets.
  - Exact explanation of the difference.
  - Final engine-calculated metrics.

---

## Step 5g — Author the opportunity record

- **Data retrieved**
  - Final calculations.
  - Distilled research.
  - House view.
  - Previous opportunity record, when applicable.

- **Model determines**
  - The investment thesis.
  - Early-detection or continuation mode.
  - Base conviction.
  - Which validated research assumptions support the forward case.
  - Catalyst description.
  - The expected thesis milestones.
  - Evidence-backed milestone date ranges.
  - Which milestone represents the thesis paying off.
  - Bear case.
  - Key falsifiers.
  - Entry consideration.
  - Proposed status:
    - New.
    - Still valid.
    - Invalidated.

- **Model restrictions**
  - Cannot invent financial numbers.
  - Cannot directly choose a price target.
  - Cannot choose a valuation multiple or discount rate.
  - Cannot assign risk tier.
  - Cannot assign horizon.
  - Cannot enforce admission gates.
  - Price action alone cannot raise conviction.
  - A research-only leading indicator can raise conviction by at most one level for any archetype.

- **Output**
  - Proposed opportunity record.
  - Proposed thesis milestone plan.

---

## Step 5h — Deterministic final validation

- **Data retrieved**
  - No new data.

- **Risk-tier calculation**
  - High risk:
    - Small company, unprofitable, highly volatile, highly leveraged, illiquid, or event-exposed.
  - Low risk:
    - Large, profitable, liquid, lower-volatility, lower-debt company.
  - Otherwise Medium.

- **Horizon calculation**
  - Short:
    - The full payoff window ends within three months.
  - Long:
    - The payoff window begins after twelve months.
    - Or multi-year compounding is the payoff.
  - Otherwise Mid.
  - The app derives the category.
  - The model supplies the evidence and timing range.

- **Entry gate**
  - Uses the research-informed target while its evidence is valid and current.
  - Otherwise uses the structured-only target.
  - Expected return must beat:
    - Low risk: `DGS2 + 8 percentage points`.
    - Medium risk: `DGS2 + 16 points`.
    - High risk: `DGS2 + 30 points`.
  - Bear downside cannot exceed base-case upside.
  - Illiquid names receive a return haircut.
  - Emerging businesses must also satisfy their double-over-horizon requirement.

- **Evidence gate**
  - Requires:
    - Current price and price history.
    - Valid leading metric.
    - Current financial or operating evidence.
    - Current bear-case evidence.
    - Computable price target.
  - Limited history does not lower these requirements.
  - Unmapped predecessor or proxy financial data is rejected.

- **Forensic logic**
  - Soft accounting warnings cap conviction at Medium.
  - Restatement, auditor change, fraud, or unsupported hype excludes a debut.

- **Validation**
  - Recalculate final conviction.
  - Confirm any conviction raise uses an independent, unscored, sourced indicator.
  - Validate milestone evidence, dates, dependencies, and completion conditions.
  - Give every machine-checkable milestone condition its own app-controlled ID.
  - An unchanged condition keeps its evaluation history when a deep pass replaces the plan.
  - A changed condition starts fresh; a milestone name alone cannot transfer history.
  - Verify model numbers match engine numbers.
  - Verify falsifiers are actually monitorable.
  - Verify source freshness.

- **Held-out candidates**
  - Gate failure → shadow gate-reject episode.
  - Missing evidence → shadow abstention episode.
  - Hard exclusion → shadow exclusion episode.

- **Existing opportunity exception**
  - Missing evidence does not remove it.
  - Its previous verdict stays.
  - A refresh gap is recorded.

- **Output**
  - Survivor with assigned matrix cell.
  - Or a typed rejection/abstention record.
  - Candidate checkpoint.

---

# Step 6 — Rank and assemble new survivors

- **Data retrieved**
  - No external data.

- **Model**
  - Ranks survivors within each predetermined cell.
  - Suggests merges for near-identical opportunities.

- **App validation**
  - A merge requires:
    - Same matrix cell.
    - Shared hypothesis, leading metric, or catalyst.
  - Invalid merge:
    - List both opportunities.
  - Existing live opportunity:
    - Cannot be merged away.
  - Every survivor must appear or have a validated merge record.

- **Output**
  - Ranked survivor matrix.
  - Validated duplicate records.
  - No fixed number of opportunities per cell.

---

# Step 7 — Refresh existing ideas and finalize the matrix

- **Data retrieved**
  - FMP prices and estimates.
  - New filing-derived metrics when available.
  - FINRA short interest when needed.
  - Stooq price history.
  - Stored opportunity and shadow episodes.

- **Deep-researched existing names**
  - Use their Step-5 result.
  - Still valid → remain in matrix.
  - Invalidated → move to archive.
  - Inconclusive → keep previous verdict.

- **All other live names**
  - Run cheap re-derivation.
  - Recalculate targets and risk tier.
  - Recheck entry gate.
  - Recheck structured falsifiers.
  - Recheck structured and filing-based milestones.
  - Refresh since-flagged performance.
  - Raise “Consider Deep Audit” when needed.
  - Never archive.

- **Final matrix logic**
  - Reinsert every surviving existing opportunity.
  - Update its risk row if risk changed.
  - Keep its last deep-pass horizon.
  - Revalidate completeness.

- **Outcome calculations**
  - Measure picked opportunities after:
    - 1 month.
    - 3 months.
    - 6 months.
    - 12 months.
  - Calculate:
    - Return.
    - Return versus sector and market.
    - Maximum drawdown.
    - Whether the leading metric continued.
    - Why the result likely occurred.

- **Shadow scorecard**
  - Measure rejected, deferred, and merged-away names.
  - Identify false negatives.
  - Never automatically promote them.

- **Graph updates**
  - Add or refresh watchlist names.
  - Retire failed hypotheses.
  - Mark archived picks as departed.

- **Model**
  - None.

- **Output**
  - Final matrix.
  - Attention warnings.
  - Archive changes.
  - Picked and shadow outcome episodes.
  - Updated opportunity graph.

---

## Step 8 — Mark opportunities you already own

- **Data retrieved**
  - Fresh holdings from Schwab.
  - Cached holdings if the pull fails.

- **Logic**
  - Add owned/not-owned labels.
  - Holdings do not affect discovery or selection.

- **Model**
  - None.

- **Output**
  - Display-only ownership tags.

---

## Step 9 — Save everything

- **Data stored**
  - Final matrix.
  - Opportunity graph.
  - Discovery coverage ledger.
  - Watchlist research-refresh state.
  - Archived opportunities.
  - Structured-only and research-informed target calculations.
  - Target assumption bridges.
  - Thesis milestone plans and their evaluation states.
  - Limited-history evidence and mapping decisions.
  - Picked and shadow episodes.
  - Outcome labels.
  - Sources and timestamps.
  - Calculations and model versions.
  - Rejection and dedup reasons.

- **Embedding model**
  - DTO:
    - Embeds opportunity summaries.
    - Embeds newly matured lessons and false negatives.
  - ATO Deep:
    - Embeds touched opportunity summaries only.
  - ATO Quick:
    - No embedding call.

- **Output**
  - Durable run record.
  - Searchable continuity memory.

---

## Step 10 — Display the result

- **Data retrieved**
  - Persisted results.
  - Cached Stooq prices for current display values.

- **UI output**
  - 3×3 risk-by-horizon matrix.
  - Optional flat sortable list.
  - Thesis, target, bear case, catalyst, and conviction.
  - Performance since first selection.
  - Owned/not-owned status.
  - Research-stale badge.
  - Consider-Deep-Audit warning.
  - Separate archive.

- **Model**
  - None.

---

# ATO: Audit selected opportunities

## Quick Audit

`Selected names → Refresh numbers → Check warnings → Save`

- **Data retrieved**
  - FMP price and estimates.
  - Stooq history.
  - FRED rates.
  - FINRA data when required.

- **Logic**
  - Recalculate targets.
  - Recheck the entry gate.
  - Recheck structured falsifiers.
  - Recheck structured and filing-based milestones.
  - Refresh performance.
  - Raise or retain warnings.

- **Model**
  - None.
  - Can run while the model server is offline.

- **Cannot**
  - Rewrite the thesis.
  - Change conviction.
  - Perform new research.
  - Archive an opportunity.

## Deep Audit

`Selected names → Full Step-5 loop → Reconcile → Save`

- **Data retrieved**
  - Full per-company data and fresh web research.

- **Model**
  - Full archetype, research, distillation, and scoring calls.

- **Can**
  - Rewrite thesis and conviction.
  - Refresh research assumptions.
  - Replace the thesis milestone plan.
  - Clear warnings after a successful pass.
  - Mark an opportunity invalidated.
  - Move an invalidated opportunity to the archive.

- **Does not**
  - Run discovery.
  - Rebuild the full watchlist.
  - Modify unrelated opportunities.

## The most important safety rule

- Fast checks may warn.
- Only fresh deep research may remove an opportunity.
- Missing data never causes automatic removal.
- A rejected candidate is still tracked for later evaluation.
