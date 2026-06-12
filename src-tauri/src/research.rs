//! Research-folder filesystem surface: list and delete user-supplied documents
//! in the research inbox and archive.
//!
//! Both folders live under the app data directory (`docs/research-documents.md`):
//! the user drops PDFs/notes into the inbox for the weekly pipeline to consider,
//! and successfully-processed documents are moved to the archive. The two share
//! this deterministic, Tauri-free core — it reads directory entries and deletes a
//! single file by name — so it can be driven directly by unit tests against temp
//! dirs. The caller passes whichever folder it means.
//!
//! Job-start parsing and the move-to-archive step (`docs/weekly-report-workflow.md`
//! §Step 6) live in `document_parser` and the pipeline's inbox stage; this module
//! stays the listing/deletion core (§User Permissions — delete yes from either
//! folder, manual archive no). The parse-failure error state the panel shows is
//! joined onto the listing by [`annotate_parse_failures`], from the
//! `research_parse_failures` table the inbox stage writes.

use std::path::{Component, Path};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::storage::ParseFailureRow;

/// Formats accepted as professional research sources
/// (`docs/research-documents.md` §Research Inbox), as lowercased extensions.
/// Files with any other extension are still listed but flagged unsupported.
const SUPPORTED_EXTENSIONS: &[&str] =
    &["pdf", "md", "markdown", "txt", "csv", "json", "html", "htm"];

/// One file in a research folder, as the UI lists it. The frontend mirror is
/// `ResearchDocument` in `src/types.ts`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResearchDocument {
    /// Bare file name including extension — never a path, since the file lives
    /// directly in the folder. This is also the key the delete command takes back.
    pub name: String,
    /// Lowercased extension, or an empty string when the file has none.
    pub format: String,
    /// Whether `format` is one the pipeline can parse.
    pub supported: bool,
    pub size_bytes: u64,
    /// Last-modified time as an RFC3339 UTC string, or `None` if the platform
    /// could not report one.
    pub modified: Option<String>,
    /// Why the last job pass could not parse this file, when it couldn't —
    /// the panel renders the row in an error state so the user can fix or
    /// delete it (`docs/research-documents.md §Parse Failures`). Populated only
    /// for the inbox listing, by [`annotate_parse_failures`]; `None` here.
    pub parse_error: Option<String>,
}

/// List the files directly in `dir`, most-recently-modified first. A missing
/// directory lists as empty (the folder is created lazily, on first reveal), so a
/// fresh install shows an empty folder rather than an error. Sub-directories and
/// dotfiles (`.DS_Store` and friends) are ignored — each research folder is a flat
/// drop of documents.
pub fn list_folder(dir: &Path) -> Result<Vec<ResearchDocument>> {
    let mut docs = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(docs),
        Err(e) => return Err(e).with_context(|| format!("reading folder {dir:?}")),
    };

    for entry in entries {
        let entry = entry.with_context(|| format!("reading an entry in {dir:?}"))?;
        // A file that vanished mid-scan: skip rather than fail the whole listing.
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let format = Path::new(&name)
            .extension()
            .map(|ext| ext.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let supported = SUPPORTED_EXTENSIONS.contains(&format.as_str());
        docs.push(ResearchDocument {
            name,
            format,
            supported,
            size_bytes: meta.len(),
            modified: modified_rfc3339(&meta),
            parse_error: None,
        });
    }

    // Newest first; entries without a modified time (None < Some) sort last,
    // with a stable name tiebreak so equal-mtime files have a deterministic order.
    docs.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.cmp(&b.name)));
    Ok(docs)
}

/// A file's mtime as an RFC3339 UTC string, or `None` when the platform can't
/// report one. The single derivation shared by the listing and the parse stage,
/// so the failure-row identity match below compares like with like.
pub fn modified_rfc3339(meta: &std::fs::Metadata) -> Option<String> {
    meta.modified()
        .ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
}

/// Join the recorded parse failures onto an inbox listing: a row gets its error
/// state only while the file on disk is identity-identical (name + size +
/// mtime) to the one that failed — an edited or replaced file reads as a fresh
/// document until the next job pass re-attempts it.
pub fn annotate_parse_failures(docs: &mut [ResearchDocument], failures: &[ParseFailureRow]) {
    for doc in docs {
        doc.parse_error = failures
            .iter()
            .find(|f| {
                f.name == doc.name
                    && f.size_bytes == doc.size_bytes
                    && f.modified == doc.modified
            })
            .map(|f| f.reason.clone());
    }
}

/// Delete one document from `dir` by file name (`docs/research-documents.md`
/// §User Permissions — the user may delete from either folder). `name` must be a
/// single bare file name: any path separator or parent reference is rejected
/// before the filesystem is touched, so a crafted name can never escape the
/// folder.
pub fn delete_folder_document(dir: &Path, name: &str) -> Result<()> {
    validate_bare_name(name)?;
    let target = dir.join(name);
    // Defense in depth: a bare name joined onto the folder must resolve to a
    // direct child of it.
    if target.parent() != Some(dir) {
        bail!("refusing to delete a path outside the folder: {name:?}");
    }
    std::fs::remove_file(&target).with_context(|| format!("deleting {target:?}"))?;
    Ok(())
}

/// Reject anything that is not a single, normal path component: empty, `.`,
/// `..`, a name containing a path separator, or any non-`Normal` component a
/// platform might parse out (a drive prefix, a root).
fn validate_bare_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("research document name is empty");
    }
    if name.contains('/') || name.contains('\\') {
        bail!("research document name must not contain a path separator: {name:?}");
    }
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => bail!("research document name must be a plain file name: {name:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn touch(dir: &Path, name: &str, contents: &[u8]) {
        let mut f = fs::File::create(dir.join(name)).unwrap();
        f.write_all(contents).unwrap();
    }

    #[test]
    fn lists_files_and_marks_supported_vs_unsupported_formats() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "note.md", b"# hi");
        touch(tmp.path(), "data.csv", b"a,b\n1,2\n");
        touch(tmp.path(), "deck.PDF", b"%PDF");
        touch(tmp.path(), "weird.xyz", b"??");
        touch(tmp.path(), "noext", b"x");

        let docs = list_folder(tmp.path()).unwrap();
        assert_eq!(docs.len(), 5);
        let by = |n: &str| docs.iter().find(|d| d.name == n).unwrap();

        assert_eq!(by("note.md").format, "md");
        assert!(by("note.md").supported);
        assert!(by("data.csv").supported);
        // Extension comparison is case-insensitive.
        assert_eq!(by("deck.PDF").format, "pdf");
        assert!(by("deck.PDF").supported);
        assert_eq!(by("weird.xyz").format, "xyz");
        assert!(!by("weird.xyz").supported);
        assert_eq!(by("noext").format, "");
        assert!(!by("noext").supported);
    }

    #[test]
    fn missing_folder_lists_as_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist");
        assert_eq!(list_folder(&missing).unwrap(), Vec::<ResearchDocument>::new());
    }

    #[test]
    fn ignores_dotfiles_and_subdirectories() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), ".DS_Store", b"x");
        fs::create_dir(tmp.path().join("nested")).unwrap();
        touch(tmp.path(), "real.txt", b"x");

        let docs = list_folder(tmp.path()).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].name, "real.txt");
    }

    #[test]
    fn delete_removes_a_named_file() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "drop.txt", b"x");
        assert!(tmp.path().join("drop.txt").exists());

        delete_folder_document(tmp.path(), "drop.txt").unwrap();
        assert!(!tmp.path().join("drop.txt").exists());
    }

    #[test]
    fn delete_rejects_traversal_and_separators_without_touching_siblings() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("folder");
        fs::create_dir(&folder).unwrap();
        // A secret one level up, outside the folder.
        touch(tmp.path(), "secret.txt", b"keep me");

        for bad in [
            "../secret.txt",
            "..",
            ".",
            "",
            "/etc/hosts",
            "sub/file.txt",
            "a\\b.txt",
        ] {
            assert!(
                delete_folder_document(&folder, bad).is_err(),
                "expected {bad:?} to be rejected"
            );
        }
        assert!(
            tmp.path().join("secret.txt").exists(),
            "a rejected name still deleted a file outside the folder"
        );
    }

    #[test]
    fn parse_failures_annotate_only_identity_matched_rows() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "broken.json", b"{ not json");
        touch(tmp.path(), "fine.md", b"# ok");
        let mut docs = list_folder(tmp.path()).unwrap();

        let failed = docs.iter().find(|d| d.name == "broken.json").unwrap();
        let failures = vec![
            ParseFailureRow {
                name: "broken.json".into(),
                size_bytes: failed.size_bytes,
                modified: failed.modified.clone(),
                reason: "the file is not valid JSON".into(),
                failed_at: "2026-06-11T09:00:00+00:00".into(),
            },
            // A stale row whose size no longer matches anything on disk.
            ParseFailureRow {
                name: "fine.md".into(),
                size_bytes: 999_999,
                modified: None,
                reason: "stale".into(),
                failed_at: "2026-06-11T09:00:00+00:00".into(),
            },
        ];
        annotate_parse_failures(&mut docs, &failures);

        let by = |n: &str| docs.iter().find(|d| d.name == n).unwrap();
        assert_eq!(
            by("broken.json").parse_error.as_deref(),
            Some("the file is not valid JSON")
        );
        assert_eq!(by("fine.md").parse_error, None, "a stale row never matches");
    }
}
