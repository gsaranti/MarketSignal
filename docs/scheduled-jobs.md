# Scheduled Jobs

This file describes the three scheduled jobs — when each one runs and what it focuses on. For how jobs actually execute (runtime preconditions, concurrency, errors, manual runs, missed-job detection), see [job-execution.md](job-execution.md).

## Premarket Report Job

Runs:
```text
Monday–Friday mornings
4:00 AM PT / 7:00 AM ET
```

Focus:
- overnight futures
- global markets
- macro calendar
- geopolitical developments
- overnight earnings/news
- expected market drivers

## Postmarket Report Job

Runs:
```text
Sunday–Friday evenings
4:00 PM PT / 7:00 PM ET
```

The Sunday run is intentional. US cash markets are closed Sunday evening, so this run is not a recap of a trading session — it prepares for the Monday session by covering weekend developments, the futures open, geopolitical news, and Asia-session signals.

Focus:
- what moved markets
- index performance
- sector leadership
- macro reactions
- yields/oil/dollar/VIX
- thesis evolution
- next-day setup

## Weekly Review Job

Runs:
```text
Saturday
6:00 PM PT / 9:00 PM ET
```

Focus:
- analyze the previous week's reports
- judge accuracy
- identify incorrect assumptions
- identify useful signals
- extract durable lessons

The weekly review is stored as a normal readable report inside the application. Its report structure and the full review process are described in [reports/weekly-review.md](reports/weekly-review.md).
