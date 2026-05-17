# Application Interface

## Main Layout

```text
Market Signal
├── Latest Report View
│   ├── Rendered HTML report
│   └── Export actions
│
├── Recent Reports Sidebar
│   ├── Ordered descending
│   ├── Report type labels
│   ├── Report timestamps
│   ├── Premarket reports
│   ├── Postmarket reports
│   └── Weekly review reports
│
├── Research Documents
│   ├── Research Inbox
│   └── Research Archive
│
├── Warning Banner Area
│   ├── Missing agent configuration
│   ├── Missing API tokens
│   ├── Failed jobs
│   └── Missed scheduled jobs
│
└── Settings
    ├── Agent model configuration
    ├── API token configuration
    ├── Scheduled job controls
    └── Manual report execution
```

## Settings

The Settings section includes:
- model selection
- API token configuration
- scheduled job enable/disable controls
- and manual job execution controls

See [agents/models.md](agents/models.md) for model selection and API token rules, [job-execution.md](job-execution.md) for the scheduled job controls and manual execution behavior.

Settings surfaces validation warnings inline — for example, when a user selects a model from a provider but has not configured the corresponding API token (see [agents/models.md](agents/models.md)).

## Warning Banner Area

The Warning Banner Area surfaces:
- Missing agent configuration
- Missing API tokens
- Failed jobs
- Missed scheduled jobs

Warning banner behavior — when banners are created, deduplicated, and how missed-job notifications are surfaced — is described in [job-execution.md](job-execution.md). Model/token validation warnings are described in [agents/models.md](agents/models.md).
