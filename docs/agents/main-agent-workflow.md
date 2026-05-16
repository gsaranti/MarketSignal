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
18. Update application UI

The main agent does not engage in recursive conversations with subagents.
It critiques responses independently during synthesis.
