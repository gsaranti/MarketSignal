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
