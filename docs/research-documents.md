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

## Processing at Job Start

At the start of each scheduled job:
1. The application checks the inbox folder.
2. If the folder is empty, the job continues normally.
3. If documents exist, the application parses them and prepares them as professional research sources.
4. Parsed document content is supplied to the report workflow and may be included in the research packet.
5. After successful processing, the application automatically moves the documents into:
```text
/research-archive
```

## User Permissions

The user may manually delete documents from either folder.
The user cannot manually archive documents.
