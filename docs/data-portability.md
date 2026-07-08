# Data Portability — Export & Import

*Status: **built** (PR #53). This spec is as-built: the archive
serializer/loader is `src-tauri/src/portability.rs`, the commands are
`export_data` / `import_data_inspect` / `import_data`, and the Settings surface
is the **Data** section plus the design package's confirmation dialog.*

Whole-corpus **backup and restore**: bundle a machine's accumulated analytical
history into a single archive and load it on another machine. The motivating
case is a hardware migration — moving everything this build has learned from an
old Mac to a new one — but the same archive doubles as an offline backup.

This is **not** the per-report export in [export.md](export.md): that saves one
report as Markdown or PDF for reading or sharing. Data portability moves the
*entire* store — every report, every durable learning, the continuity state —
as one restorable unit. The two features are unrelated beyond the word "export";
a reader wanting a single report's PDF is in the wrong document.

## The problem it solves

The store is stable across app *versions* on one machine — the data dir is keyed
by bundle identifier, so a rebuild reads the same data ([storage.md
§Storage Location](storage.md#storage-location)). It is **not** portable across
*machines*: a fresh install on new hardware starts empty, and there is no path
to carry 30 reports, the accumulated durable learnings, and the cross-report
continuity state over. Re-generating that history is impossible (it is grounded
in past market states) and re-earning the learnings would take months of runs.
Data portability is the missing bridge.

## What moves, and what deliberately does not

The line is **durable analytical data moves; secrets and machine-local
operational state stay behind.** Concretely, mapping onto the actual stores
([storage.md](storage.md)):

**Exported — SQLite tables** (serialized as rows, not a file copy — see
[§Why a structured archive](#why-a-structured-archive-not-a-db-file-copy)):

| Table | Why it moves |
|---|---|
| `reports` | The report records — id, regime metadata, summary JSON, the pointer to the Markdown body. |
| `baseline_snapshots` | Past market-scan states (retention 14). **Irreproducible** — they capture a market moment; they anchor the cadence-honest change view on the next report. |
| `vector_memory` | Report summaries **and durable learnings** — the long-term semantic memory. Durable learnings are the point: they survive report deletion and are the accumulated edge. |
| `portfolio_runs` | Local-suite run history (retention 10). Nascent today, but durable once the suite runs live. |
| `holdings_pulls` | The single latest view-only holdings snapshot. |

**Exported — filesystem stores:**

- `reports/` — the **canonical Markdown bodies**. This is load-bearing: the
  report *content* is not in SQLite. The DB holds only a `markdown_path` pointer
  and the summary JSON, so a table-only export would carry 30 empty shells
  pointing at files that do not exist on the target. The files must ride along.
- `research-archive/` — processed source documents.
- `research-inbox/` — documents awaiting processing.

**Excluded, by construction:**

| Not exported | Where it lives | Why it stays |
|---|---|---|
| API keys, tokens, model selection, Schwab client id | `app_settings` (plaintext) | Secrets and machine config. Re-entered once on the target — a fresh install already prompts for them. The exporter never serializes this table, so secrets cannot leak into the archive. |
| Schwab app secret + OAuth tokens | macOS Keychain | Never portable; the token lifecycle is a weekly re-login on any machine ([schwab-integration.md](schwab-integration.md)). |
| Light/Dark preference | webview `localStorage` | Pure presentation, no backend consumer. |
| `job_runs` | SQLite | Machine-local operational history — run outcomes, not analytical product. |
| `research_parse_failures`, `document_truncations`, `document_parse_runs` | SQLite | Regenerable per-report telemetry, not primary data. |
| HTML | (nothing — rendered on demand) | Never stored anywhere ([storage.md §SQLite](storage.md#sqlite)). |

The exclusion of `app_settings` and the Keychain is the whole security story:
**nothing sensitive is ever placed in the archive**, so even an unencrypted
export cannot spill a credential.

## The archive

A single **`.zip`**, structured and self-describing:

```text
market-signal-export-YYYY-MM-DD.zip
  manifest.json              format version · app version · created-at (UTC) ·
                             per-table row counts · per-namespace embedder id ·
                             file inventory + checksums · encrypted flag
  db/
    reports.ndjson
    baseline_snapshots.ndjson
    vector_memory.ndjson     content + embedding + namespace + kind + report_id
    portfolio_runs.ndjson
    holdings_pulls.ndjson
  reports/                   the canonical Markdown bodies
  research-archive/          processed source documents
  research-inbox/            pending source documents
```

The default export filename drops the per-report `-<id8>` suffix convention —
this is a corpus archive, not a report file — and a same-name collision is the
save dialog's own overwrite prompt.

### Why a structured archive, not a DB-file copy

Copying `market_signal.db` wholesale looks simpler but is wrong on three counts:

1. **WAL sidecars.** The database runs in WAL mode, so `market_signal.db-wal`
   and `market_signal.db-shm` sit beside it; a naive copy of just the main file
   captures a torn state.
2. **Secrets ride along.** A binary copy carries the `app_settings` table —
   every plaintext key — with no clean way to strip it. Serializing row-by-row
   lets the exporter simply never touch that table.
3. **No schema-version marker to lean on.** The DB has no `PRAGMA user_version`
   and no migrations table; schema evolution is idempotent `CREATE TABLE IF NOT
   EXISTS` plus guarded `ALTER … ADD COLUMN`. A raw file from an old build
   dropped onto a new one is brittle. A structured import instead runs the
   target's own schema-init first, then inserts — so it adapts to whatever
   schema the target build carries. The archive stamps its **own** format
   version in the manifest to gate gross incompatibility.

A structured archive is also human-inspectable (open the zip, read your reports
as Markdown) and lets import re-embed vectors if it ever needs to
(see [§Vector memory is embedder-bound](#vector-memory-is-embedder-bound)).

### Optional passphrase encryption

The archive contents are non-secret by the exclusion rules above, but they are
still the user's **entire market-analysis history** — and, once the local suite
runs live, portfolio holdings and verdicts derived from a Schwab account. That
is sensitive personal financial data in the clear.

Export therefore offers **optional passphrase encryption** (as built:
AES-256-GCM over an Argon2id-derived key, wrapping the whole container).
Unchecked, the export is a plain `.zip` with a UI
warning to keep the file private and move it over a trusted channel. Checked, the
whole archive is encrypted; import detects an encrypted container and prompts for
the passphrase. There is **no recovery path** — a lost passphrase means an
unrecoverable archive, stated plainly at export time.

## Export flow

A new **Data** section in Settings, with an **Export** action:

1. User optionally enters a passphrase (leave blank for plaintext).
2. Tauri **save** dialog picks the destination (default
   `market-signal-export-YYYY-MM-DD.zip`).
3. The backend `export_data` command builds the archive entirely in Rust
   (the app layer owns all I/O — the agents/spine boundary is untouched):
   serialize the included tables to NDJSON, copy the report and research files,
   write the manifest with checksums, zip, and encrypt if a passphrase was set.
4. Success surfaces the path and a count of what was written (N reports, N
   learnings, N files).

Export takes no model call and is not a pipeline job, but it **must not run
concurrently with a report or local-suite job** — a mid-run export could capture
a half-written state. It **claims the single run slot**
(`RunKind::DataPortability`) for the whole command, dialog included — so a job
can neither be running when the archive is cut nor start mid-archive — and the
Settings buttons disable while anything holds the slot.

## Import flow

The **Import** action in the same Settings section:

1. Tauri **open** dialog picks a `.zip`.
2. If the container is encrypted, it decrypts with the Data section's shared
   passphrase field; absent or wrong, import stops with a typed error asking for
   it (no dedicated prompt dialog — retrying reopens the picker).
3. Read and validate the manifest: reject a format version newer than this build
   understands; verify every entry's size + checksum, and never consume bytes
   the manifest doesn't list. All row-level validation — NDJSON parse, embedding
   decode, the schema's uniqueness/cardinality — also runs here, **before any
   destructive step**, so a bad archive can only abort while the store is
   untouched.
4. Determine whether the target store is **empty** (no reports, no learnings, no
   portfolio runs):
   - **Empty** (the hardware-migration case) → straight load.
   - **Non-empty** → a confirmation modal makes the destructive scope explicit:
     *"This replaces all existing reports, learnings, snapshots, and portfolio
     runs. Your API keys and settings are untouched. Continue?"* On confirm, the
     included tables and the report/research directories are cleared, then
     loaded. **Merge is deliberately deferred** (see below).
5. Load, in dependency order so foreign keys resolve (reports before the
   `vector_memory` summaries and `baseline_snapshots` that reference them):
   1. Run the idempotent schema-init so every table and column exists.
   2. Insert the table rows.
   3. Copy the Markdown and research files into place, and **re-derive each
      `markdown_path`** to the target's own absolute path — the exported path was
      machine-specific (a bundle-id data dir) and must never be trusted verbatim.
   4. A report record whose Markdown file is missing from the archive is skipped
      with a logged warning, never imported as a dangling shell — its summary
      vector row and baseline snapshots drop with it, mirroring the live
      deletion cascade.
6. `app_settings` is **never read or written** by import — the target machine's
   keys and config are left exactly as they were.
7. Refresh in place: every store-reading surface (reports list + pane, portfolio
   state, research folders, warnings, job status) re-fetches after the load. No
   restart is needed — commands open the database per call, and the backend
   keeps no cross-command cache to go stale.

Retention needs no special handling: the archive was produced by a machine that
already enforced the caps (30 reports, 14 snapshots, 10 portfolio runs), so it
arrives within them and the next run's retention pass is a no-op.

### Why replace-all, not merge (for v1)

The motivating case — a new Mac — imports into an **empty** store, where load and
replace are identical. Merge (insert-or-skip by `report_id` / `run_id`, dedup
learnings by content hash) adds real complexity and edge cases for a scenario v1
does not need. The archive is designed with stable primary keys so merge is a
clean later addition, but v1 ships **fresh-load / replace-with-confirmation** and
defers merge.

### Vector memory is embedder-bound

Each `vector_memory` row stores its **source `content` alongside the embedding**,
and the store carries no embedder-identity column — dimensionality is implicit in
the vector, and search skips rows whose dimension does not match the query. This
has one consequence for import:

- **Report-namespace vectors** embed with OpenAI `text-embedding-3-large` — a
  fixed internal model, non-configurable and identical on every machine because
  it is a cloud API. These are the migration payload, and they import and work
  unchanged.
- **Local-suite namespaces** (`portfolio`, `opportunities`) embed with the local
  model. If the target machine's configured local embedder differs from the one
  that produced them, those vectors no longer match its query dimension and are
  silently skipped — harmless, not corrupting. Because the row keeps its
  `content`, a future re-embed pass can regenerate them, and the manifest records
  the per-namespace embedder id so import can detect the mismatch.

**This does not affect the M5 hardware migration.** Two independent reasons: the
report-namespace vectors that make up the payload use the fixed cloud embedder
above, so they cannot diverge; and the local-suite namespaces are *empty* on any
machine that has not run the suite live — the local models are hardware-gated, so
an old Mac accumulates no local vectors to carry, and the new machine builds its
local vector memory fresh the first time the suite runs there (correct and
unavoidable regardless of export/import). The embedder-mismatch case is scoped
strictly to a **later** scenario: exporting from a machine that *has* run the
suite live, into one configured with a **different local embedder model** — the
"change the local embedding model" case — and even then it degrades to a
re-embed, never data loss.

## Verification

Per [CLAUDE.md](../CLAUDE.md) the full suite runs with the slice:

- **Backend** — `cargo test` (the `portability` module's round-trip suite is
  the core: export a seeded temp store, import it into a second temp dir,
  assert row-and-file parity for every included table/folder — incl. the
  re-derived `markdown_path` — assert `app_settings` never reaches the archive,
  the encrypted + wrong-passphrase round-trips, and the tamper cases: bad
  checksums, manifest-unlisted entries, corrupt embeddings, and duplicate rows,
  each aborting before the destructive phase) plus
  `cargo clippy --all-targets --all-features`.
- **Frontend** — `npm run build`, a Settings spec for the Data section
  (export/import emit payloads, the passphrase field, status/error channels,
  disabled-while-a-job-runs), and a ConfirmDialog spec (a11y contract, initial
  focus on Cancel, Escape/scrim cancel inert while busy, the busy state).

The Settings surface follows the existing Settings patterns and the design
package; robustness (the confirmation modal, error and disabled states, the
passphrase field's keyboard/screen-reader behavior) is `frontend-craft` work.

## Build-order placement

This is **independent of the local suite** — it moves whatever the store holds,
so it works today against the cloud report corpus and automatically covers the
Portfolio / Trade-Opportunities tables as they fill in (the exporter is
table-driven). It landed (PR #53) **before** the M5 transition, so the
accumulated report history and learnings survive the move.
