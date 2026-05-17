# Research Document Workflow

The application contains two local folders:
```text
/research-inbox
/research-archive
```

## Research Inbox

The user can manually place documents into:
```text
/research-inbox
```

Supported formats:
- PDF
- Markdown
- TXT
- CSV
- JSON
- HTML

At the start of each scheduled job:
1. The main agent checks the inbox folder.
2. If the folder is empty, the job continues normally.
3. If documents exist, they are parsed and treated as professional research sources.
4. The documents are incorporated into the current research process.
5. After successful processing, the documents are automatically moved into:
```text
/research-archive
```

The user may manually delete documents from either folder.
The user cannot manually archive documents.

## Failure Handling

Each file in `/research-inbox` is processed independently. A file fails processing when it has an unsupported extension, is malformed or corrupted, or otherwise cannot be parsed.

When a file fails:
- the file is skipped and not incorporated into the research process
- the file is left in `/research-inbox` (not moved to `/research-archive`)
- the failure is logged

The job continues processing the remaining files and proceeds normally — a single failed file does not cancel the job.

If one or more files failed processing during a job, the application displays a warning banner listing the filenames that failed. The warning follows the same deduplication and dismissal rules as other warnings (see [job-execution.md](job-execution.md)).
