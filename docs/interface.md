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
│   ├── Report timestamps
│   └── Weekly Market reports
│
├── Research Documents
│   ├── Research Inbox
│   └── Research Archive
│
├── Persistent Warning Area
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

The operational behavior of each panel is defined in the relevant concern files:
- Latest Report View / Recent Reports Sidebar — see [report-structure.md](report-structure.md) and [storage.md](storage.md).
- Export actions — see [export.md](export.md).
- Research Documents (Inbox / Archive) — see [research-documents.md](research-documents.md).
- Persistent Warning Area triggers — see [scheduling.md](scheduling.md) and [configuration.md](configuration.md).
- Settings — see [configuration.md](configuration.md) and [scheduling.md](scheduling.md).
