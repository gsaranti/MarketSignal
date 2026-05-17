# Main Agent Workflow

## Full Flow

1. Scheduled job starts
2. Load settings
3. Load recent Markdown reports and structured metadata
4. Query vector memory
5. Check research inbox
6. Gather baseline market data
7. Gather news and research
8. Perform dynamic research branching
9. Build condensed research packet
10. Send packet to Bull/Bear/Balanced subagents
11. Receive subagent theses
12. Critique subagent responses independently
13. Synthesize final report
14. Save Markdown report to SQLite
15. Save report summary to vector DB
16. Save durable learnings if applicable
17. Generate HTML report from Markdown
18. Save HTML to SQLite
19. Update application UI

The main agent does not engage in recursive conversations with subagents.
It critiques responses independently during synthesis.

## Memory Retrieval

At step 4, the main agent queries LanceDB for semantically relevant memory entries. The query is constructed from the current job's baseline-scan topics (see [../research-behavior.md](../research-behavior.md)) and the prior report's thesis summary.

Retrieval pulls the top 10 nearest report summaries and the top 10 nearest durable learnings by cosine similarity. The two entry types are defined in [../storage.md](../storage.md).

The retrieved entries are injected into the main agent's prompt as a "Relevant memory" block before synthesis. This injection is the only path by which long-term memory enters the main agent's context, consistent with the bounded-injection rule in [../cost-control.md](../cost-control.md).

## Subagent Invocation

At step 10, the Bull, Bear, and Balanced subagents are invoked in parallel for latency. The main agent waits for all three responses before proceeding to step 11.

All three subagent responses are required. If any subagent invocation fails — API error, timeout, malformed response, or model execution error — the entire job fails per the [error handling rules](../job-execution.md#error-handling). The main agent does not synthesize a report from a partial set of subagent responses.
