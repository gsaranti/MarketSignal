//! Step-6 research-inbox parsing (`docs/weekly-report-workflow.md §Step 6`,
//! `docs/research-documents.md`): turn the user-supplied files in the research
//! inbox into bounded text the research router and the condensed packet can
//! carry.
//!
//! The whole stage is deterministic — full text is extracted per format, then
//! condensed by pure char-cap rules rather than a model. `docs/agents.md §Data
//! Extraction` records this decision (amended 2026-06-12): no model extraction
//! stage runs; a GPT-5-mini stage stays reserved as the named follow-on if
//! oversized documents prove common — it would replace [`truncate_at_seam`]'s
//! "take the head" for docs that overflow [`PER_DOC_CHAR_CAP`], with nothing
//! else changing.
//!
//! Failure semantics (`docs/research-documents.md §Parse Failures`): a document
//! that cannot be parsed never fails the job — it is skipped with a recorded
//! reason, left in the inbox for the next run, and surfaced in the Research
//! Documents panel via the `research_parse_failures` table (`storage`). Only
//! successfully parsed documents are archived, and only after the run's report
//! persists (`pipeline`'s persist step) — a failed or cancelled run leaves the
//! inbox untouched.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use crate::progress::RunContext;
use crate::research::{self, ResearchDocument};

/// Hard ceiling on a file the stage will read at all. A multi-hundred-MB drop
/// would cost memory and parse time for text that the char budgets below
/// discard anyway; oversize reads as a parse failure the user can see.
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;

/// Per-document ceiling on condensed text, in chars (~3k tokens). Sized so
/// typical user drops — saved articles, notes, short reports — ride whole;
/// only genuinely long documents are head-truncated, visibly. Tunable alongside
/// the packet caps.
const PER_DOC_CHAR_CAP: usize = 12_000;

/// Total budget across all documents, in chars (~10k tokens). Water-filled:
/// small documents keep their full text and donate their surplus to larger
/// ones, so three short notes plus one big PDF means the notes ride whole and
/// the PDF takes nearly the whole remainder. Bounds what the condensed packet
/// (and through it four agent calls) can carry, whatever the inbox holds.
const TOTAL_CHAR_BUDGET: usize = 40_000;

/// Per-document ceiling on the shorter excerpt handed to research routing —
/// the router picks topics, it does not deep-read.
const ROUTER_EXCERPT_CHARS: usize = 2_000;

/// Data rows kept from a CSV (the header row always survives); the remainder
/// is summarized as a count so the agent knows the sample is a sample.
const CSV_MAX_DATA_ROWS: usize = 100;

/// Render width for the HTML→text pass. Wide enough that prose lines rarely
/// hard-wrap mid-sentence; paragraph breaks (blank lines) are what the
/// truncation seam keys on, and those survive any width.
const HTML_RENDER_WIDTH: usize = 100;

/// When truncating, back off from the hard cut to the last paragraph (or line)
/// seam — but only if that seam keeps at least this fraction of the window,
/// so a document with one giant paragraph still yields a usable head.
const SEAM_FLOOR: f64 = 0.7;

/// One successfully parsed inbox document: the listing identity it was parsed
/// from (`name`/`size_bytes`/`modified`, used to skip the archive move if the
/// file changed underneath the run) plus the normalized, budget-condensed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedResearchDoc {
    pub name: String,
    /// Lowercased extension, as `research::list_folder` reports it.
    pub format: String,
    pub size_bytes: u64,
    /// RFC3339 UTC mtime at parse time, or `None` when the platform could not
    /// report one — the same derivation as the listing, so the two compare.
    pub modified: Option<String>,
    /// The condensed text: normalized full text, head-truncated to this doc's
    /// share of the char budgets.
    pub text: String,
    /// Char count of the normalized *full* text, so truncation stays visible.
    pub original_chars: usize,
}

impl ParsedResearchDoc {
    /// Whether `text` is a head-cut of a longer original.
    pub fn truncated(&self) -> bool {
        self.text.chars().count() < self.original_chars
    }

    /// The full prompt block the condensed packet carries: a provenance header,
    /// an explicit truncation marker when the text is a head-cut (the gaps-manifest
    /// posture — the agent knows it is seeing part of a document), and the text.
    pub fn prompt_block(&self) -> String {
        let mut block = format!("### Research document: {}", self.header_suffix());
        if self.truncated() {
            block.push_str(&format!(
                "\n[truncated — showing the first {} of {} characters]",
                self.text.chars().count(),
                self.original_chars
            ));
        }
        block.push_str("\n\n");
        block.push_str(&self.text);
        block
    }

    /// The shorter routing excerpt: the same header over the head of the
    /// condensed text, capped at [`ROUTER_EXCERPT_CHARS`].
    pub fn router_excerpt(&self) -> String {
        let head: String = self.text.chars().take(ROUTER_EXCERPT_CHARS).collect();
        let cut = head.chars().count() < self.text.chars().count();
        format!(
            "### Research document: {}\n\n{}{}",
            self.header_suffix(),
            head,
            if cut { "…" } else { "" }
        )
    }

    /// `name (FORMAT, modified YYYY-MM-DD)` — the provenance line shared by
    /// both renderings.
    fn header_suffix(&self) -> String {
        match self.modified.as_deref().and_then(|m| m.get(..10)) {
            Some(date) => format!(
                "{} ({}, modified {})",
                self.name,
                self.format.to_uppercase(),
                date
            ),
            None => format!("{} ({})", self.name, self.format.to_uppercase()),
        }
    }
}

/// One document that could not be parsed: the listing identity (the panel's
/// error state matches on it) and a short human-readable reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseFailure {
    pub name: String,
    pub size_bytes: u64,
    pub modified: Option<String>,
    pub reason: String,
}

/// What one inbox pass produced: the parsed documents (condensed, prompt-ready)
/// and the failures (left in the inbox, recorded for the panel).
#[derive(Debug, Default)]
pub struct InboxOutcome {
    pub docs: Vec<ParsedResearchDoc>,
    pub failures: Vec<ParseFailure>,
}

/// Parse every supported document in the inbox, newest first. Fully fail-soft:
/// an unlistable folder degrades to an empty outcome with a stderr note, a bad
/// file becomes a [`ParseFailure`] rather than an error, and unsupported
/// formats are skipped entirely (the panel already tags them; they are not
/// parse failures). Cancellation is polled per file, mirroring the executor's
/// per-request polling — a cancel stops further parsing, and the caller skips
/// the failure write so a partial pass never clobbers the recorded state.
pub fn process_inbox(dir: &Path, ctx: &RunContext) -> InboxOutcome {
    let listing = match research::list_folder(dir) {
        Ok(listing) => listing,
        Err(e) => {
            eprintln!("research-inbox: listing degraded to empty: {e:#}");
            return InboxOutcome::default();
        }
    };

    let mut full_texts: Vec<(ResearchDocument, String)> = Vec::new();
    let mut failures = Vec::new();
    for doc in listing {
        if ctx.is_cancelled() {
            break;
        }
        if !doc.supported {
            continue;
        }
        match parse_document(dir, &doc) {
            Ok(text) => full_texts.push((doc, text)),
            Err(reason) => failures.push(ParseFailure {
                name: doc.name,
                size_bytes: doc.size_bytes,
                modified: doc.modified,
                reason,
            }),
        }
    }

    InboxOutcome {
        docs: condense_documents(full_texts),
        failures,
    }
}

/// Move each successfully parsed document into the archive
/// (`docs/research-documents.md §Processing at Job Start` — archiving is
/// automatic and reserved for successfully processed documents). Best-effort
/// per file, like the persist step's other legs: a failed move logs to stderr
/// and leaves the file for the next run's idempotent re-parse. A file whose
/// size or mtime changed since parse time is skipped — the run consumed the
/// old contents, so the new ones deserve their own pass.
pub fn archive_processed(inbox_dir: &Path, archive_dir: &Path, docs: &[ParsedResearchDoc]) {
    if docs.is_empty() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(archive_dir) {
        eprintln!("research-inbox: creating archive directory {archive_dir:?} failed: {e}");
        return;
    }
    for doc in docs {
        let src = inbox_dir.join(&doc.name);
        let meta = match std::fs::metadata(&src) {
            Ok(meta) => meta,
            Err(e) => {
                eprintln!(
                    "research-inbox: {} vanished before archiving: {e}",
                    doc.name
                );
                continue;
            }
        };
        let modified = research::modified_rfc3339(&meta);
        if meta.len() != doc.size_bytes || modified != doc.modified {
            eprintln!(
                "research-inbox: {} changed since it was parsed; leaving it for the next run",
                doc.name
            );
            continue;
        }
        let dest = match unique_destination(archive_dir, &doc.name) {
            Some(dest) => dest,
            None => {
                eprintln!(
                    "research-inbox: no free archive name for {}; leaving it in the inbox",
                    doc.name
                );
                continue;
            }
        };
        if let Err(e) = std::fs::rename(&src, &dest) {
            eprintln!("research-inbox: archiving {} failed: {e}", doc.name);
        }
    }
}

/// `name`, or `stem (2).ext`, `stem (3).ext`, … — the first archive path not
/// already taken. `None` once the counter is exhausted (a pathological archive).
fn unique_destination(archive_dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = archive_dir.join(name);
    if !direct.exists() {
        return Some(direct);
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    };
    for n in 2..1000 {
        let candidate = match ext {
            Some(ext) => archive_dir.join(format!("{stem} ({n}).{ext}")),
            None => archive_dir.join(format!("{stem} ({n})")),
        };
        if !candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Read and parse one listed document to its normalized full text. `Err` is a
/// short human-readable reason — it shows verbatim in the panel's error state.
fn parse_document(dir: &Path, doc: &ResearchDocument) -> Result<String, String> {
    if doc.size_bytes > MAX_FILE_BYTES {
        return Err(format!(
            "file is too large to parse ({} MB; the limit is {} MB)",
            doc.size_bytes / (1024 * 1024),
            MAX_FILE_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(dir.join(&doc.name)).map_err(|e| format!("reading the file: {e}"))?;
    let raw = extract_text(&doc.format, bytes)?;
    let text = normalize_text(&raw);
    if text.is_empty() {
        return Err("no extractable text".to_string());
    }
    Ok(text)
}

/// Per-format extraction to raw text. Formats are the supported set from
/// `docs/research-documents.md §Research Inbox`; the caller has already
/// filtered to them, so an unknown format here is a programming error surfaced
/// as a parse failure rather than a panic.
fn extract_text(format: &str, bytes: Vec<u8>) -> Result<String, String> {
    match format {
        "md" | "markdown" | "txt" => {
            String::from_utf8(bytes).map_err(|_| "the file is not valid UTF-8 text".to_string())
        }
        "csv" => {
            let text = String::from_utf8(bytes)
                .map_err(|_| "the file is not valid UTF-8 text".to_string())?;
            Ok(condense_csv(&text))
        }
        "json" => {
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|e| format!("the file is not valid JSON: {e}"))?;
            serde_json::to_string_pretty(&value)
                .map_err(|e| format!("re-rendering the JSON failed: {e}"))
        }
        "html" | "htm" => html2text::from_read(bytes.as_slice(), HTML_RENDER_WIDTH)
            .map_err(|e| format!("parsing the HTML failed: {e}")),
        "pdf" => extract_pdf_text(&bytes),
        other => Err(format!("unsupported format {other:?}")),
    }
}

/// PDF text extraction with panic containment: `pdf-extract` is known to panic
/// on some malformed inputs (unwraps, overflow in char-code conversion), and
/// these are user-supplied files — so the call runs under `catch_unwind` and a
/// panic reads as a parse failure, never a crashed run. (An unwinding panic;
/// the rarer unbounded-recursion abort class is not catchable in-process.)
fn extract_pdf_text(bytes: &[u8]) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem(bytes)
    })) {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(e)) => Err(format!("extracting PDF text failed: {e}")),
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            Err(format!("the PDF could not be parsed ({msg})"))
        }
    }
}

/// Header record + the first [`CSV_MAX_DATA_ROWS`] data records, with the
/// remainder summarized as a count — the agent sees the schema and a sample
/// rather than a random cut. Record-aware, not line-based: a quoted field may
/// span lines (RFC 4180), so records split on newlines *outside* quotes, the
/// cap never cuts mid-field, and the omitted count counts records. Deliberately
/// lenient about malformation — CSV has no enforceable grammar and real-world
/// files are routinely ragged, so any UTF-8 text is accepted as evidence (an
/// unbalanced quote is implicitly closed at end of input) rather than surfaced
/// as a parse failure; that leniency is a conscious call, unlike JSON's strict
/// validation.
fn condense_csv(text: &str) -> String {
    let records = split_csv_records(text);
    if records.len() <= CSV_MAX_DATA_ROWS + 1 {
        return text.to_string();
    }
    let kept = &records[..CSV_MAX_DATA_ROWS + 1];
    let omitted = records.len() - kept.len();
    let mut out = kept.join("\n");
    out.push('\n');
    out.push_str(&format!("… {omitted} more rows"));
    out
}

/// Split CSV text into records: a newline inside a quoted field belongs to its
/// record. A `"` toggles the quoted state — the doubled `""` escape is two
/// toggles, which nets correctly for boundary purposes (field-level parsing is
/// not needed here). Trailing `\r` is trimmed per record so CRLF input joins
/// back as clean LF; a trailing newline yields no empty final record.
fn split_csv_records(text: &str) -> Vec<&str> {
    let mut records = Vec::new();
    let mut in_quotes = false;
    let mut start = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            '\n' if !in_quotes => {
                let end = if i > start && text.as_bytes()[i - 1] == b'\r' {
                    i - 1
                } else {
                    i
                };
                records.push(&text[start..end]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < text.len() {
        let rest = text[start..].strip_suffix('\r').unwrap_or(&text[start..]);
        records.push(rest);
    }
    records
}

/// Normalize extracted text for prompt use: CRLF→LF, control chars (except
/// newline/tab) dropped, runs of spaces/tabs collapsed to one space, lines
/// trimmed at **both** ends (leading indentation is dropped too — nested-list
/// and code-block indentation flattens, an accepted loss for prompt
/// condensation), runs of 3+ newlines collapsed to a paragraph break, and the
/// whole trimmed. PDF extraction especially produces ragged spacing; this buys
/// back tokens without losing a word.
fn normalize_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut pending_space = false;
    let mut newline_run = 0usize;
    for c in raw.replace("\r\n", "\n").replace('\r', "\n").chars() {
        match c {
            '\n' => {
                // Dropping `pending_space` here is what trims trailing spaces.
                pending_space = false;
                newline_run += 1;
                if newline_run <= 2 {
                    out.push('\n');
                }
            }
            c if c == ' ' || c == '\t' => {
                if !out.is_empty() && newline_run == 0 {
                    pending_space = true;
                }
            }
            c if c.is_control() => {}
            c => {
                if pending_space {
                    out.push(' ');
                    pending_space = false;
                }
                newline_run = 0;
                out.push(c);
            }
        }
    }
    out.trim().to_string()
}

/// Apply the per-doc cap and the water-filled total budget, preserving the
/// caller's (newest-first) order. Pure: same inbox in, same texts out.
fn condense_documents(full: Vec<(ResearchDocument, String)>) -> Vec<ParsedResearchDoc> {
    let desired: Vec<usize> = full
        .iter()
        .map(|(_, text)| text.chars().count().min(PER_DOC_CHAR_CAP))
        .collect();
    let allocations = water_fill(&desired, TOTAL_CHAR_BUDGET);
    full.into_iter()
        .zip(allocations)
        .map(|((doc, text), allocation)| {
            let original_chars = text.chars().count();
            ParsedResearchDoc {
                name: doc.name,
                format: doc.format,
                size_bytes: doc.size_bytes,
                modified: doc.modified,
                text: truncate_at_seam(&text, allocation),
                original_chars,
            }
        })
        .collect()
}

/// Split `budget` chars across documents wanting `desired[i]` each: when the
/// total fits, everyone gets their ask; when it doesn't, smaller documents are
/// satisfied in full first and the rest split the remainder evenly — so short
/// notes ride whole and only the large documents are cut. Deterministic, no
/// reordering of the output.
fn water_fill(desired: &[usize], budget: usize) -> Vec<usize> {
    if desired.iter().sum::<usize>() <= budget {
        return desired.to_vec();
    }
    let mut order: Vec<usize> = (0..desired.len()).collect();
    order.sort_by_key(|&i| desired[i]);
    let mut allocations = vec![0usize; desired.len()];
    let mut remaining_budget = budget;
    let mut remaining_docs = desired.len();
    for &i in &order {
        let share = remaining_budget / remaining_docs;
        let give = desired[i].min(share);
        allocations[i] = give;
        remaining_budget -= give;
        remaining_docs -= 1;
    }
    allocations
}

/// Head-truncate `text` to at most `max_chars`, backing the cut off to the last
/// paragraph break — or failing that, the last line break — inside the window,
/// provided the seam keeps at least [`SEAM_FLOOR`] of it. A document with no
/// usable seam is hard-cut at a char boundary instead.
fn truncate_at_seam(text: &str, max_chars: usize) -> String {
    let window: String = text.chars().take(max_chars).collect();
    if window.chars().count() >= text.chars().count() {
        return window;
    }
    let floor = (max_chars as f64 * SEAM_FLOOR) as usize;
    for seam in ["\n\n", "\n"] {
        if let Some(pos) = window.rfind(seam) {
            if window[..pos].chars().count() >= floor {
                return window[..pos].trim_end().to_string();
            }
        }
    }
    window.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn ctx() -> std::sync::Arc<RunContext> {
        // `process_inbox` takes `&RunContext`; the noop never cancels.
        RunContext::noop()
    }

    fn write(dir: &Path, name: &str, contents: &[u8]) {
        std::fs::write(dir.join(name), contents).unwrap();
    }

    /// A minimal, structurally valid one-page PDF whose content stream draws
    /// `text` in Helvetica — offsets and stream length computed so the xref is
    /// real, not hand-guessed.
    fn minimal_pdf(text: &str) -> Vec<u8> {
        let stream = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >>"
                .to_string(),
            format!(
                "<< /Length {} >>\nstream\n{stream}\nendstream",
                stream.len()
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
        }
        let xref_at = pdf.len();
        pdf.push_str(&format!(
            "xref\n0 {}\n0000000000 65535 f \n",
            objects.len() + 1
        ));
        for off in offsets {
            pdf.push_str(&format!("{off:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        ));
        pdf.into_bytes()
    }

    // ---- per-format extraction ----

    #[test]
    fn markdown_and_txt_parse_as_utf8_text() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "note.md",
            b"# Fed outlook\n\nRates likely hold.",
        );
        let out = process_inbox(tmp.path(), &ctx());
        assert!(out.failures.is_empty());
        assert_eq!(out.docs.len(), 1);
        assert_eq!(out.docs[0].text, "# Fed outlook\n\nRates likely hold.");
        assert!(!out.docs[0].truncated());
    }

    #[test]
    fn invalid_utf8_is_a_parse_failure() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "bad.txt", &[0xff, 0xfe, 0x00, 0x41]);
        let out = process_inbox(tmp.path(), &ctx());
        assert!(out.docs.is_empty());
        assert_eq!(out.failures.len(), 1);
        assert!(
            out.failures[0].reason.contains("UTF-8"),
            "{}",
            out.failures[0].reason
        );
    }

    #[test]
    fn valid_json_is_validated_and_rendered() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "data.json",
            br#"{"thesis":"hold","horizon_weeks":6}"#,
        );
        let out = process_inbox(tmp.path(), &ctx());
        assert!(out.failures.is_empty());
        assert!(out.docs[0].text.contains("\"thesis\": \"hold\""));
    }

    #[test]
    fn malformed_json_is_a_parse_failure() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "broken.json", b"{ not json");
        let out = process_inbox(tmp.path(), &ctx());
        assert!(out.docs.is_empty());
        assert!(
            out.failures[0].reason.contains("not valid JSON"),
            "{}",
            out.failures[0].reason
        );
    }

    #[test]
    fn csv_keeps_header_and_caps_data_rows() {
        let mut csv = String::from("date,close\n");
        for i in 0..(CSV_MAX_DATA_ROWS + 40) {
            csv.push_str(&format!("2026-01-01,{i}\n"));
        }
        let condensed = condense_csv(&csv);
        let lines: Vec<&str> = condensed.lines().collect();
        assert_eq!(lines[0], "date,close");
        assert_eq!(lines.len(), CSV_MAX_DATA_ROWS + 2, "header + cap + marker");
        assert_eq!(*lines.last().unwrap(), "… 40 more rows");
        // A small CSV is untouched.
        assert_eq!(condense_csv("a,b\n1,2\n"), "a,b\n1,2\n");
    }

    #[test]
    fn csv_quoted_multiline_fields_count_as_records_not_lines() {
        // A quoted field spanning lines is one record (RFC 4180): a small file
        // with one such record stays untouched rather than reading as over-cap.
        let small = "id,note\n1,\"line one\nline two\nline three\"\n";
        assert_eq!(condense_csv(small), small);

        // Over the cap, the omitted count counts records, and the cut never
        // lands inside a quoted field.
        let mut csv = String::from("id,note\n");
        for i in 0..(CSV_MAX_DATA_ROWS + 5) {
            csv.push_str(&format!("{i},\"first\nsecond\"\n"));
        }
        let condensed = condense_csv(&csv);
        assert!(
            condensed.ends_with("… 5 more rows"),
            "records counted, not lines"
        );
        let body = condensed.strip_suffix("… 5 more rows").unwrap();
        assert_eq!(
            body.matches('"').count() % 2,
            0,
            "no record was cut mid-field"
        );
        // Header + 100 two-line records survive intact.
        assert!(
            body.starts_with("id,note\n0,\"first\nsecond\"\n"),
            "sample keeps whole records"
        );
        assert!(
            body.contains(&format!("\n{},\"first\nsecond\"\n", CSV_MAX_DATA_ROWS - 1)),
            "the last kept record is the capped one"
        );
    }

    #[test]
    fn html_is_stripped_to_readable_text() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "saved.html",
            b"<html><body><h1>Credit stress</h1><p>Spreads <b>widened</b> sharply.</p>\
              <script>alert('x')</script></body></html>",
        );
        let out = process_inbox(tmp.path(), &ctx());
        assert!(out.failures.is_empty());
        let text = &out.docs[0].text;
        assert!(text.contains("Credit stress"), "{text}");
        assert!(text.contains("widened"), "{text}");
        assert!(!text.contains("<p>"), "tags stripped: {text}");
        assert!(!text.contains("alert"), "script content dropped: {text}");
    }

    #[test]
    fn pdf_text_is_extracted_from_a_valid_pdf() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "deck.pdf", &minimal_pdf("Market thesis intact"));
        let out = process_inbox(tmp.path(), &ctx());
        assert!(out.failures.is_empty(), "{:?}", out.failures);
        assert!(
            out.docs[0].text.contains("Market thesis intact"),
            "{}",
            out.docs[0].text
        );
    }

    #[test]
    fn garbage_pdf_is_a_parse_failure_not_a_crash() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "junk.pdf", b"%PDF-1.4 this is not really a pdf");
        let out = process_inbox(tmp.path(), &ctx());
        assert!(out.docs.is_empty());
        assert_eq!(out.failures.len(), 1);
        assert!(!out.failures[0].reason.is_empty());
    }

    #[test]
    fn empty_and_oversize_files_are_parse_failures() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "empty.txt", b"   \n  ");
        let out = process_inbox(tmp.path(), &ctx());
        assert!(out.failures[0].reason.contains("no extractable text"));

        // The oversize guard is checked before the read, off the listed size.
        let doc = ResearchDocument {
            name: "huge.txt".into(),
            format: "txt".into(),
            supported: true,
            size_bytes: MAX_FILE_BYTES + 1,
            modified: None,
            parse_error: None,
        };
        let err = parse_document(tmp.path(), &doc).unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn unsupported_files_are_skipped_entirely() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "image.png", b"\x89PNG");
        let out = process_inbox(tmp.path(), &ctx());
        assert!(out.docs.is_empty());
        assert!(
            out.failures.is_empty(),
            "unsupported is not a parse failure"
        );
    }

    #[test]
    fn a_cancelled_pass_parses_nothing() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        use crate::progress::NoopReporter;

        // The per-file poll is checked before each file, so a cancel that is
        // already set parses (and records) nothing — even a file that would
        // have failed.
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "note.md", b"# fine");
        write(tmp.path(), "broken.json", b"{ not json");
        let cancelled =
            RunContext::new("t", Arc::new(NoopReporter), Arc::new(AtomicBool::new(true)));
        let out = process_inbox(tmp.path(), &cancelled);
        assert!(out.docs.is_empty());
        assert!(
            out.failures.is_empty(),
            "no failure is recorded for unreached files"
        );
    }

    // ---- normalization ----

    #[test]
    fn normalize_collapses_whitespace_and_drops_control_chars() {
        let raw = "A  ragged\t\tline \nsecond\u{0}\u{7} line\r\n\n\n\n\nnext paragraph  ";
        assert_eq!(
            normalize_text(raw),
            "A ragged line\nsecond line\n\nnext paragraph"
        );
    }

    // ---- condensing ----

    #[test]
    fn water_fill_satisfies_small_docs_and_splits_the_rest() {
        // Two small docs ride whole; the two large ones split the remainder evenly.
        let desired = vec![1_000, 12_000, 500, 12_000];
        let allocs = water_fill(&desired, 10_000);
        assert_eq!(allocs[0], 1_000);
        assert_eq!(allocs[2], 500);
        assert_eq!(allocs[1] + allocs[3], 8_500);
        assert!(allocs[1].abs_diff(allocs[3]) <= 1);
        // Under budget: everyone gets their ask.
        assert_eq!(water_fill(&[100, 200], 1_000), vec![100, 200]);
    }

    #[test]
    fn truncation_cuts_at_a_paragraph_seam_and_is_marked() {
        let text = format!("{}\n\n{}", "a".repeat(900), "b".repeat(900));
        let cut = truncate_at_seam(&text, 1_000);
        assert_eq!(cut, "a".repeat(900), "backed off to the paragraph seam");

        // No seam inside the floor -> hard cut at the cap.
        let solid = "x".repeat(2_000);
        assert_eq!(truncate_at_seam(&solid, 1_000).chars().count(), 1_000);

        let doc = ParsedResearchDoc {
            name: "big.txt".into(),
            format: "txt".into(),
            size_bytes: 2_000,
            modified: Some("2026-06-09T12:00:00+00:00".into()),
            text: cut,
            original_chars: text.chars().count(),
        };
        assert!(doc.truncated());
        let block = doc.prompt_block();
        assert!(block.starts_with("### Research document: big.txt (TXT, modified 2026-06-09)"));
        assert!(
            block.contains("[truncated — showing the first 900 of 1802 characters]"),
            "{block}"
        );
    }

    #[test]
    fn untruncated_docs_carry_no_marker() {
        let doc = ParsedResearchDoc {
            name: "note.md".into(),
            format: "md".into(),
            size_bytes: 10,
            modified: None,
            text: "short".into(),
            original_chars: 5,
        };
        let block = doc.prompt_block();
        assert_eq!(block, "### Research document: note.md (MD)\n\nshort");
        assert_eq!(
            doc.router_excerpt(),
            block,
            "short docs ride whole in both forms"
        );
    }

    #[test]
    fn router_excerpt_is_capped_with_an_ellipsis() {
        let doc = ParsedResearchDoc {
            name: "big.txt".into(),
            format: "txt".into(),
            size_bytes: 0,
            modified: None,
            text: "y".repeat(ROUTER_EXCERPT_CHARS * 2),
            original_chars: ROUTER_EXCERPT_CHARS * 2,
        };
        let excerpt = doc.router_excerpt();
        assert!(excerpt.ends_with('…'));
        assert!(excerpt.chars().count() < doc.text.chars().count());
    }

    // ---- archive move ----

    #[test]
    fn archive_moves_parsed_files_and_suffixes_collisions() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = tmp.path().join("inbox");
        let archive = tmp.path().join("archive");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::create_dir_all(&archive).unwrap();
        write(&inbox, "note.md", b"fresh");
        write(&archive, "note.md", b"already archived");

        let meta = std::fs::metadata(inbox.join("note.md")).unwrap();
        let docs = vec![ParsedResearchDoc {
            name: "note.md".into(),
            format: "md".into(),
            size_bytes: meta.len(),
            modified: research::modified_rfc3339(&meta),
            text: "fresh".into(),
            original_chars: 5,
        }];
        archive_processed(&inbox, &archive, &docs);

        assert!(!inbox.join("note.md").exists(), "moved out of the inbox");
        assert!(
            archive.join("note.md").exists(),
            "the pre-existing file is untouched"
        );
        assert_eq!(
            std::fs::read_to_string(archive.join("note (2).md")).unwrap(),
            "fresh",
            "the collision landed under a suffixed name"
        );
    }

    #[test]
    fn archive_skips_a_file_that_changed_since_parse() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = tmp.path().join("inbox");
        let archive = tmp.path().join("archive");
        std::fs::create_dir_all(&inbox).unwrap();
        write(&inbox, "note.md", b"contents that changed after the parse");

        let docs = vec![ParsedResearchDoc {
            name: "note.md".into(),
            format: "md".into(),
            size_bytes: 5, // does not match the file on disk
            modified: None,
            text: "stale".into(),
            original_chars: 5,
        }];
        archive_processed(&inbox, &archive, &docs);
        assert!(
            inbox.join("note.md").exists(),
            "a changed file stays for the next run"
        );
    }
}
