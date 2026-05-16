# Cost-Control Architecture

The application is designed with bounded workflows to prevent excessive token usage.

## News Ingestion Flow

The system does not send large raw news volumes into frontier models.

Pipeline:
```text
~500 headlines gathered
→ deduplication
→ relevance scoring
→ clustering
→ ~40 relevant headlines
→ ~10 important stories
→ ~5 deeply analyzed topics
```

## Context Window Control

The application does not repeatedly inject large historical report histories into prompts.

Instead:
- only a bounded subset of recent Markdown reports is loaded into agent context
- vector memory retrieval is used selectively
- only relevant memory fragments are injected into prompts

## Agent Workflow Limits

The application enforces:
- bounded research depth
- bounded retries
- bounded subagent execution
- no recursive agent loops
- no recursive debate cycles
