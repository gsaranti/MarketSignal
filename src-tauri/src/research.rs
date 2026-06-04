//! Research-inbox filesystem surface: list and delete user-supplied documents.
//!
//! The research inbox is a plain folder under the app data directory where the
//! user drops PDFs/notes for the weekly pipeline to consider
//! (`docs/research-documents.md`). This module is the deterministic, Tauri-free
//! core — it reads directory entries and deletes a single file by name — so it
//! can be driven directly by unit tests against temp dirs.
//!
//! Job-start parsing and the move-to-archive step (`docs/weekly-report-workflow.md`
//! §Step 5) are not implemented yet, so there is no parse-failure error state to
//! surface here: this lists what is on disk and lets the user delete it
//! (§User Permissions — delete yes, archive no).

use std::path::{Component, Path};

use anyhow::{bail, Context, Result};
use serde::Serialize;

/// Formats the inbox accepts as professional research sources
/// (`docs/research-documents.md` §Research Inbox), as lowercased extensions.
/// Files with any other extension are still listed but flagged unsupported.
const SUPPORTED_EXTENSIONS: &[&str] =
    &["pdf", "md", "markdown", "txt", "csv", "json", "html", "htm"];

/// One file in the research inbox, as the UI lists it. The frontend mirror is
/// `ResearchDocument` in `src/types.ts`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResearchDocument {
    /// Bare file name including extension — never a path, since the file lives
    /// directly in the inbox folder. This is also the key the delete command
    /// takes back.
    pub name: String,
    /// Lowercased extension, or an empty string when the file has none.
    pub format: String,
    /// Whether `format` is one the pipeline can parse.
    pub supported: bool,
    pub size_bytes: u64,
    /// Last-modified time as an RFC3339 UTC string, or `None` if the platform
    /// could not report one.
    pub modified: Option<String>,
}

/// List the files directly in `inbox_dir`, most-recently-modified first. A
/// missing directory lists as empty (the folder is created lazily, on first
/// reveal), so a fresh install shows an empty inbox rather than an error.
/// Sub-directories and dotfiles (`.DS_Store` and friends) are ignored — the
/// inbox is a flat drop of documents.
pub fn list_inbox(inbox_dir: &Path) -> Result<Vec<ResearchDocument>> {
    let mut docs = Vec::new();
    let entries = match std::fs::read_dir(inbox_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(docs),
        Err(e) => return Err(e).with_context(|| format!("reading inbox {inbox_dir:?}")),
    };

    for entry in entries {
        let entry = entry.with_context(|| format!("reading an entry in {inbox_dir:?}"))?;
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
        let modified = meta
            .modified()
            .ok()
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
        docs.push(ResearchDocument {
            name,
            format,
            supported,
            size_bytes: meta.len(),
            modified,
        });
    }

    // Newest first; entries without a modified time (None < Some) sort last,
    // with a stable name tiebreak so equal-mtime files have a deterministic order.
    docs.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.cmp(&b.name)));
    Ok(docs)
}

/// Delete one document from the inbox by file name (`docs/research-documents.md`
/// §User Permissions). `name` must be a single bare file name: any path
/// separator or parent reference is rejected before the filesystem is touched,
/// so a crafted name can never escape the inbox directory.
pub fn delete_inbox_document(inbox_dir: &Path, name: &str) -> Result<()> {
    validate_bare_name(name)?;
    let target = inbox_dir.join(name);
    // Defense in depth: a bare name joined onto the inbox must resolve to a
    // direct child of the inbox.
    if target.parent() != Some(inbox_dir) {
        bail!("refusing to delete a path outside the inbox: {name:?}");
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

        let docs = list_inbox(tmp.path()).unwrap();
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
    fn missing_inbox_lists_as_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist");
        assert_eq!(list_inbox(&missing).unwrap(), Vec::<ResearchDocument>::new());
    }

    #[test]
    fn ignores_dotfiles_and_subdirectories() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), ".DS_Store", b"x");
        fs::create_dir(tmp.path().join("nested")).unwrap();
        touch(tmp.path(), "real.txt", b"x");

        let docs = list_inbox(tmp.path()).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].name, "real.txt");
    }

    #[test]
    fn delete_removes_a_named_file() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "drop.txt", b"x");
        assert!(tmp.path().join("drop.txt").exists());

        delete_inbox_document(tmp.path(), "drop.txt").unwrap();
        assert!(!tmp.path().join("drop.txt").exists());
    }

    #[test]
    fn delete_rejects_traversal_and_separators_without_touching_siblings() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = tmp.path().join("inbox");
        fs::create_dir(&inbox).unwrap();
        // A secret one level up, outside the inbox.
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
                delete_inbox_document(&inbox, bad).is_err(),
                "expected {bad:?} to be rejected"
            );
        }
        assert!(
            tmp.path().join("secret.txt").exists(),
            "a rejected name still deleted a file outside the inbox"
        );
    }
}
