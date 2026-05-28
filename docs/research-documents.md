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

## Parse Failures

Research documents are optional supplements, so a document that cannot be parsed never fails the job. When a supported-format file is malformed or otherwise cannot be parsed:
- the application skips it and logs the failure
- the job continues with the remaining documents and the rest of the workflow
- the file is left in `/research-inbox`; it is not moved to `/research-archive`, which is reserved for successfully processed documents
- the application surfaces the failure in the Research Documents panel (the file is shown in an error state) so the user can fix or delete it

Because an unparseable file remains in the inbox, it is re-attempted on the next job unless the user deletes it (see [User Permissions](#user-permissions)).

## User Permissions

The user may manually delete documents from either folder.
The user cannot manually archive documents.
