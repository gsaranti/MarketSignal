# SEC earnings recovery fixtures

Retrieved from the public SEC archive on 2026-09-19 for Slice 3 parser and extraction regressions.
The HTML files retain the original response bodies, including SEC document wrappers and inline-XBRL markup, except for trailing-whitespace cleanup in the filing index.
The submissions fixture retains the real issuer metadata and all parallel-array fields for one filing; other filings and historical-file pointers are omitted to make the positive case deterministic.
Tests add competing filings, amendments and malformed metadata explicitly.

| Fixture | Source |
| --- | --- |
| `psx-submissions.json` | https://data.sec.gov/submissions/CIK0001534701.json |
| `psx-index.htm` | https://www.sec.gov/Archives/edgar/data/1534701/000153470126000030/0001534701-26-000030-index.htm |
| `psx-8k.htm` | https://www.sec.gov/Archives/edgar/data/1534701/000153470126000030/psx-20260805.htm |
| `psx-ex991.htm` | https://www.sec.gov/Archives/edgar/data/1534701/000153470126000030/psx-2026630_ex991.htm |
| `tsla-ex991.htm` | https://www.sec.gov/Archives/edgar/data/1318605/000162828026049213/exhibit991.htm |

The PSX wire fixture exercises the production fetcher, failure memory, discovery parsers, extraction, same-pass synthesis admission, source-ID resolution and SEC citation identity.
The extraction regression asserts PSX's `96%` utilization and `$3.8 billion` earnings text and Tesla's `$0.4B` operating income and `$1.1B` net income text.
Text recovery from the Tesla exhibit does not imply OCR or recovery of every chart/image, and does not make an opaque IR request eligible for automatic matching.
These fixtures establish offline behavior, not future SEC availability or live-model compliance.
