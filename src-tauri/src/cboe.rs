//! Cboe daily put/call statistics — the run-level **venue-level
//! options-sentiment backdrop** (`docs/data-sources.md §CBOE`): the exchange's
//! own daily put/call ratios (total / index / equity), a broad-market context
//! signal, never a per-name one. Keyless and wholly fail-soft.
//!
//! **This adapter reads JSON embedded in the served page, not an API.** No
//! machine-readable current-day endpoint exists (the cdn.cboe.com ratio CSVs
//! froze at 2019-10-04), and the daily page is a client-rendered app whose raw
//! response carries the day's statistics as an **embedded JSON payload** —
//! `"name":"TOTAL PUT/CALL RATIO","value":"0.80"` rows plus a
//! `"selectedDate"` (live-verified 2026-08-20 against the served markup; the
//! legacy `/us/options/market_statistics/daily/` URL redirects here). The
//! extraction is a bounded scan for those labels and the date key, escaping
//! variants tolerated. Every LOCALLY detectable form of structure drift —
//! wrong key or label shape, wrong quote representation, disagreeing
//! same-form candidates — fails the parse into the caller's typed gap; the
//! feed is optional by contract, so a broken extraction degrades the
//! backdrop, never the run. The guarantee is exactly that strong and no
//! stronger: parsing the payload whole would mean first extracting and
//! reassembling the chunk-split script stream — a heavier retrieval
//! strategy declined by ruling 2026-08-21 for this optional feed — so
//! within the bounded scan a well-formed sole candidate in the document's own
//! form reads as the payload wherever it sits. That sole-impostor case is
//! the accepted residual risk, sized to the backdrop's optional
//! venue-context role ([`QuoteForm`]'s doc carries the mechanism-level
//! statement).

use std::sync::Arc;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result};

use crate::progress::RunContext;

/// The daily statistics page (the current canonical URL — the legacy
/// `/us/options/market_statistics/daily/` path redirects here). The backdrop
/// wants the venue's latest posted day, so no date parameter is sent.
const CBOE_DAILY_URL: &str = "https://www.cboe.com/markets/us/options/market-statistics/daily";

const CBOE_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// The venue-level put/call backdrop: Cboe's own posted ratios for its latest
/// trading day. `as_of` is the date label the extraction reads, verbatim —
/// the page decides which session it shows, never a local clock — carried
/// under the module-level guarantee scope and its sole-impostor residual.
#[derive(Debug, Clone, PartialEq)]
pub struct PutCallBackdrop {
    pub as_of: String,
    pub total: Option<f64>,
    pub index: Option<f64>,
    pub equity: Option<f64>,
}

/// Live Cboe daily-statistics adapter. Keyless; no execution-gate presence.
pub struct CboeDataSource {
    http: reqwest::blocking::Client,
    url: String,
    progress: Arc<RunContext>,
}

impl CboeDataSource {
    pub fn new() -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(CBOE_TIMEOUT)
            .build()
            .context("building the Cboe HTTP client")?;
        Ok(Self {
            http,
            url: CBOE_DAILY_URL.to_string(),
            progress: RunContext::noop(),
        })
    }

    /// Redirect at a localhost mock so the wire path runs offline. Test-only.
    #[cfg(test)]
    fn with_url(mut self, url: &str) -> Self {
        self.url = url.to_string();
        self
    }

    /// Attach a live run context so the fetch streams a tracker row.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Fetch and extract the latest daily put/call backdrop. `Err` is the
    /// caller's typed gap (transport, non-2xx, or a page whose embedded
    /// payload no longer parses — structure drift, not emptiness).
    pub fn put_call_backdrop(&self) -> Result<PutCallBackdrop> {
        if self.progress.is_cancelled() {
            anyhow::bail!("CBOE fetch skipped (run cancelled)");
        }
        self.progress
            .request_started("CBOE", "put-call-daily", "daily", "CBOE put/call backdrop");
        let result = (|| -> Result<PutCallBackdrop> {
            let (status, body) =
                crate::http_retry::send_with_retry("CBOE", || self.http.get(&self.url))?;
            if !(200..300).contains(&status) {
                anyhow::bail!("Cboe daily statistics returned {status}");
            }
            parse_daily_stats(&body)
        })();
        match &result {
            Ok(_) => self.progress.request_finished(
                "CBOE",
                "put-call-daily",
                "daily",
                "CBOE put/call backdrop",
                "ok",
                None,
            ),
            Err(e) => self.progress.request_finished(
                "CBOE",
                "put-call-daily",
                "daily",
                "CBOE put/call backdrop",
                "failed",
                Some(e.to_string()),
            ),
        }
        result
    }
}

/// Extract the backdrop from the served page's embedded JSON payload. Pure, so
/// the extraction contract is fixture-testable offline. Requires the
/// `selectedDate` key and at least one parsed ratio — a page serving neither is
/// drift, `Err`, never an empty success.
fn parse_daily_stats(html: &str) -> Result<PutCallBackdrop> {
    let (as_of, form) = date_after_key(html, "selectedDate")
        .context("Cboe daily page: no selectedDate in the embedded payload — structure drift")?;
    let ratio = |label: &str| ratio_near_label(html, label, form);
    let backdrop = PutCallBackdrop {
        as_of,
        total: ratio("TOTAL PUT/CALL RATIO"),
        index: ratio("INDEX PUT/CALL RATIO"),
        equity: ratio("EQUITY PUT/CALL RATIO"),
    };
    if backdrop.total.is_none() && backdrop.index.is_none() && backdrop.equity.is_none() {
        anyhow::bail!("Cboe daily page: no put/call ratio parsed — structure drift");
    }
    Ok(backdrop)
}

/// The first **calendar-valid** `YYYY-MM-DD` date inside a short window after
/// `key` — escaping variants (`\"selectedDate\":\"…\"` vs plain JSON) don't
/// matter, since only the date shape is matched, and the shape is then parsed
/// so an impossible date ("2026-99-99") reads as drift, never an as-of.
/// Returns the date **with the [`QuoteForm`] its key opened in**: the date is
/// the payload's one mandatory anchor, so its form classifies the whole
/// document and every ratio read must match it (round 9).
fn date_after_key(html: &str, key: &str) -> Option<(String, QuoteForm)> {
    let mut candidates: Vec<(Option<String>, QuoteForm)> = Vec::new();
    let mut from = 0;
    while let Some(found) = html[from..].find(key) {
        let at = from + found;
        let rest = &html[at + key.len()..];
        from = at + key.len();
        // Exact key, quoted on BOTH sides IN ONE FORM: the key's own opening
        // quote classifies the representation — plain vs escaped payload —
        // and rejects prefix extensions ("fallbackselectedDate"); the close
        // must agree with that form, rejecting suffix extensions
        // ("selectedDateTime") and, since round 8, a plain-form literal
        // quote posing as the boundary (Codex rounds 4–5, 8).
        let Some(form) = quote_form_opening_at(&html[..at]) else {
            continue;
        };
        if !closes_string(rest, form) {
            continue;
        }
        // Bind to the key's IMMEDIATE scalar — skip only the JSON / escaping
        // punctuation between key and value, then require a calendar-valid
        // date shape at that exact position. Scanning any further would let a
        // null / drifted `selectedDate` borrow a nearby valid date
        // (`"minDate"`) as the as-of (Codex 2026-08-20 rounds 3–4).
        let start = scalar_start(rest);
        let Some(cand) = rest.get(start..start + 10) else {
            candidates.push((None, form));
            continue;
        };
        // The date must be the WHOLE scalar: its closing quote must follow
        // (a datetime under this key is a changed schema — drift, not an
        // as-of; the same full-scalar rule the ratios hold, round 6). The
        // close must be an actual quote in the key's own form (round 8) — a bare
        // backslash CONTINUES the scalar (a `\u0020`-style escape), and
        // input ending at the date is truncation — so neither terminates
        // (round 7).
        let terminated = rest
            .get(start + 10..)
            .is_some_and(|r| closes_string(r, form));
        let c = cand.as_bytes();
        let digits = |r: std::ops::Range<usize>| r.into_iter().all(|k| c[k].is_ascii_digit());
        let valid = terminated
            && digits(0..4) && c[4] == b'-' && digits(5..7) && c[7] == b'-' && digits(8..10)
            && chrono::NaiveDate::parse_from_str(cand, "%Y-%m-%d").is_ok();
        candidates.push((valid.then(|| cand.to_string()), form));
    }
    // The document must be UNANIMOUS about its as-of (round 10): every
    // anchored key is a candidate — an unreadable scalar as `None` — and
    // any disagreement on date or form (a second same-form `selectedDate`
    // carrying another day) leaves no honest as-of to serve, so the whole
    // read is `None` and the feed a typed gap. First-key-wins was retired
    // with this: it silently preferred whichever candidate came first.
    let (date, form) = candidates.first()?.clone();
    if !candidates.iter().all(|c| c.0 == date && c.1 == form) {
        return None;
    }
    Some((date?, form))
}

/// Whether the text ending at `at` — through only key / scalar punctuation —
/// closes a properly quoted `name` key: the label at `at` must be the row's own
/// `"name"` value, never an unrelated field's text (Codex round 5). The strip
/// must consume at least the `":"` punctuation between key and value.
fn label_is_name_value(html: &str, at: usize) -> bool {
    let before = &html[..at];
    let trimmed = before.trim_end_matches(|c: char| {
        matches!(c, '"' | '\\' | ':' | ' ' | '\t' | '\n' | '\r')
    });
    if trimmed.len() == before.len() {
        return false;
    }
    match trimmed.strip_suffix("name") {
        Some(stripped) => stripped.ends_with('"'),
        None => false,
    }
}

/// The offset of the first character that is not JSON / escaping punctuation
/// between a key and its scalar (quotes, backslashes, colons, whitespace).
fn scalar_start(s: &str) -> usize {
    s.find(|c: char| !matches!(c, '"' | '\\' | ':' | ' ' | '\t' | '\n' | '\r'))
        .unwrap_or(s.len())
}

/// The payload's quote representation at one anchor site. The embedded rows
/// arrive either as plain JSON or stringified inside the page's script
/// (`\"name\":\"…\"`), and the SAME two bytes `\"` mean opposite things in
/// the two forms — the string delimiter in the escaped payload, a literal
/// quote INSIDE the current string in plain JSON (Codex round 8). Each
/// anchor therefore classifies its own form from its opening quote and
/// demands every quote it consumes — the label close, the value key, the
/// scalar close — agree with that form; a mixed read degrades to a gap.
/// The forms also agree RECORD-WIDE (round 9): the mandatory date key
/// classifies the whole document, and every ratio must be served in that
/// same representation — so a stringified row quoted inside a plain
/// document, or a plain row beside the escaped payload, can never join the
/// backdrop its date anchors. Same-form candidates are then cross-checked
/// for UNANIMITY rather than located structurally (round 10) — the payload
/// is chunk-split, so container binding cannot survive an arbitrary chunk
/// boundary — leaving one deliberate residue: a same-form fake that is a
/// label's ONLY candidate, or that agrees with the real row, still reads,
/// because nothing local distinguishes it from a relocated payload.
#[derive(Clone, Copy, PartialEq)]
enum QuoteForm {
    Plain,
    Escaped,
}

/// Whether `s` ends exactly where a string OPENS in `form` — a delimiter
/// for the text that follows, never a literal quote inside a string (`\"`
/// in plain JSON, `\\\"` in the escaped payload — one escaping level
/// deeper each), so in-string text shaped like a key cannot anchor.
fn opens_string(s: &str, form: QuoteForm) -> bool {
    match form {
        QuoteForm::Plain => s.ends_with('"') && !s.ends_with("\\\""),
        QuoteForm::Escaped => s.ends_with("\\\"") && !s.ends_with("\\\\\""),
    }
}

/// The representation whose string-open delimiter `s` ends with, if either
/// (the guards in [`opens_string`] make the two mutually exclusive).
fn quote_form_opening_at(s: &str) -> Option<QuoteForm> {
    [QuoteForm::Escaped, QuoteForm::Plain]
        .into_iter()
        .find(|f| opens_string(s, *f))
}

/// Whether `s` begins exactly where a string CLOSES in `form`. Plain: the
/// bare quote alone — a `\"` here is a literal quote CONTINUING the string
/// (round 8; the round-7 predicate accepted it in both forms at once).
/// Escaped: `\"` — a literal quote arrives as `\\\"`, which starts with a
/// doubled backslash and never matches. A bare backslash is a close in
/// NEITHER form (an escape continues the string, round 7), and empty input
/// is truncation, never termination.
fn closes_string(s: &str, form: QuoteForm) -> bool {
    match form {
        QuoteForm::Plain => s.starts_with('"'),
        QuoteForm::Escaped => s.starts_with("\\\""),
    }
}

/// The ratio for `label` — each anchored row contributes the first finite
/// number **after its own `value` key** (inside a short bounded window), and
/// the served value is the UNANIMOUS candidate across every anchored row.
/// The `value` anchor is load-bearing: the embedded rows read
/// `"name":"<LABEL>","value":"0.80"`, and taking the first number after the
/// label alone would let a drifted row ("…,\"rank\":1,\"value\":\"0.80\"")
/// serve a plausible fabricated ratio (Codex 2026-08-20 round 2, finding 2).
fn ratio_near_label(html: &str, label: &str, form: QuoteForm) -> Option<f64> {
    let mut candidates: Vec<Option<f64>> = Vec::new();
    let mut from = 0;
    while let Some(found) = html[from..].find(label) {
        let label_at = from + found;
        let start = label_at + label.len();
        // The label must be the row's own `name` value — a label appearing as
        // some unrelated field's text ("note":"TOTAL PUT/CALL RATIO") must not
        // anchor a ratio read (Codex 2026-08-20 round 5).
        if !label_is_name_value(html, label_at) {
            from = start;
            continue;
        }
        // The label must open in the DOCUMENT's form — the one the mandatory
        // date key classified (round 9) — and every quote consumed below
        // must agree with it (round 8): the same two bytes are the delimiter
        // in one form and in-string content in the other, so an
        // opposite-form row-shaped text (a stringified row quoted inside a
        // plain document, or plain text beside the escaped payload) can
        // never join the backdrop the date anchors.
        if !opens_string(&html[..label_at], form) {
            from = start;
            continue;
        }
        // `get` so a window edge landing inside a multibyte char degrades to
        // an empty window rather than a slice panic.
        let window = html
            .get(start..html.len().min(start + 60))
            .or_else(|| html.get(start..))
            .unwrap_or("");
        // The label must also END the name value — its closing quote, in the
        // row's own form, must follow immediately: "TOTAL PUT/CALL RATIO
        // 30-DAY" is a different product's label (Codex 2026-08-20 round 6),
        // an escaped CONTINUATION is no close (a bare backslash starts an
        // escape, round 7), and neither is a plain-form literal quote — the
        // label keeps going past it (round 8).
        if !closes_string(window, form) {
            from = start;
            continue;
        }
        // Anchor only in the window's BRACE-FREE head. The served rows carry
        // `value` immediately after the name (`"name":"<LABEL>","value":"…"`),
        // so a brace of ANY kind before the key — the row's own close, a
        // nested object opening ("meta":{…}), or a brace inside a string
        // value — means the row's own top-level value is not there to read.
        // This replaced brace-depth counting (round 7): the counter mistook a
        // `}` inside a string for a row close, letting a NESTED object's
        // `value` read back at apparent top level; a region containing no
        // braces at all cannot be fooled that way, and an in-string brace can
        // only shrink it — a gap, never a fabricated read.
        let head = window.find(['{', '}']).map_or(window, |i| &window[..i]);
        // Every `value` key in the head, first one whose IMMEDIATE scalar is
        // a whole number wins — the key string must close right after `value`
        // and open right before it: `"value":null` degrades, and `valueType`
        // / `value30` / `fallbackvalue` are different keys, never anchors
        // (rounds 3–5). The scalar itself is read from the full window, so a
        // bare number closed by the row's `}` still terminates.
        let mut row_value = None;
        let mut w = 0;
        while let Some(v_at) = head[w..].find("value") {
            let abs = w + v_at;
            let after = &window[abs + "value".len()..];
            if opens_string(&head[..abs], form) && closes_string(after, form) {
                let skip = scalar_start(after);
                // Two quotes between key and scalar (the key's close plus the
                // scalar's open, in either escaping form) mean a QUOTED
                // scalar, which must end at a closing quote; one quote means
                // a bare JSON number, closed by structural punctuation.
                let quoted = after[..skip].matches('"').count() >= 2;
                if let Some(v) = leading_number(&after[skip..], quoted, form) {
                    row_value = Some(v);
                    break;
                }
            }
            w += v_at + "value".len();
        }
        candidates.push(row_value);
        from = start;
    }
    // The document must be UNANIMOUS about this label (round 10): every
    // anchored same-form row is a candidate — a valueless one as `None` —
    // and any disagreement (a decoy object's number beside the collection's
    // own null, or two rows carrying different ratios) gives the scanner no
    // structural way to tell which container is operative, so the label
    // degrades to a gap. First-row-wins was retired with this: it silently
    // preferred whichever candidate came first.
    let first = *candidates.first()?;
    if candidates.iter().all(|c| *c == first) {
        first
    } else {
        None
    }
}

/// The number at the very START of `s` (`digits[.digits]`), parsed finite —
/// `None` when `s` does not begin with a digit, **or when the digits do not
/// make up the whole scalar**, so `"1e2"` and `"0.80oops"` are drift, never
/// `1.0` / `0.80` (Codex 2026-08-20 round 6). What closes the scalar depends
/// on its form (rounds 7–8): a `quoted` scalar must end at a closing
/// quote — in-string whitespace ("0.80 percent") or an escape continues the
/// string — while a bare JSON number ends at structural punctuation or
/// whitespace. Either way the close must be PRESENT: the bounded window
/// ending mid-number is a truncated scalar, never a value.
fn leading_number(s: &str, quoted: bool, form: QuoteForm) -> Option<f64> {
    let b = s.as_bytes();
    if !b.first().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    let end = b
        .iter()
        .position(|c| !(c.is_ascii_digit() || *c == b'.'))
        .unwrap_or(b.len());
    let terminated = if quoted {
        closes_string(&s[end..], form)
    } else {
        s[end..].starts_with([',', '}', ']', ' ', '\t', '\n', '\r'])
    };
    if !terminated {
        return None;
    }
    s[..end]
        .trim_end_matches('.')
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    /// A minimal fixture in the served shape: the ratios and date ride an
    /// escaped JSON payload inside a script chunk (the live page's form,
    /// captured 2026-08-20).
    const DAILY_PAGE: &str = r#"<html><body><script>self.__next_f.push([1,"{\"pcRatios\":[{\"name\":\"TOTAL PUT/CALL RATIO\",\"value\":\"0.80\"},{\"name\":\"INDEX PUT/CALL RATIO\",\"value\":\"0.97\"},{\"name\":\"EQUITY PUT/CALL RATIO\",\"value\":\"0.52\"}],\"selectedDate\":\"2026-08-19\"}"])</script></body></html>"#;

    #[test]
    fn parses_the_embedded_payload_labels_and_date() {
        let b = parse_daily_stats(DAILY_PAGE).unwrap();
        assert_eq!(b.as_of, "2026-08-19");
        assert_eq!(b.total, Some(0.80));
        assert_eq!(b.index, Some(0.97));
        assert_eq!(b.equity, Some(0.52));
        // Plain (unescaped) JSON parses identically — the scan matches shapes,
        // not an escaping convention.
        let plain = r#"{"pcRatios":[{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}],"selectedDate":"2026-08-19"}"#;
        let b = parse_daily_stats(plain).unwrap();
        assert_eq!(b.as_of, "2026-08-19");
        assert_eq!(b.equity, Some(0.52));
        assert_eq!(b.total, None);
    }

    #[test]
    fn structure_drift_errors_rather_than_reading_empty() {
        // No selectedDate key.
        assert!(parse_daily_stats(r#"{"name":"TOTAL PUT/CALL RATIO","value":"0.80"}"#).is_err());
        // A date key without a date-shaped value.
        assert!(parse_daily_stats(r#"{"selectedDate":"soon"}"#).is_err());
        // A date-shaped but calendar-impossible value is drift, not an as-of.
        assert!(parse_daily_stats(
            r#"{"selectedDate":"2026-99-99","name":"TOTAL PUT/CALL RATIO","value":"0.80"}"#
        )
        .is_err());
        // A date but no ratio label parses to a number.
        assert!(parse_daily_stats(r#"{"selectedDate":"2026-08-19","note":"volume only"}"#).is_err());
    }

    #[test]
    fn drifted_immediate_scalars_degrade_never_borrow_neighbors() {
        // `"value":null` must not borrow the next number in the window …
        let null_value = r#"{"selectedDate":"2026-08-19","pcRatios":[
            {"name":"TOTAL PUT/CALL RATIO","value":null,"rank":1},
            {"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}"#;
        let b = parse_daily_stats(null_value).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // … a `valueType` key must not serve its own digits as a ratio …
        let value_type = r#"{"selectedDate":"2026-08-19","pcRatios":[
            {"name":"TOTAL PUT/CALL RATIO","valueType":"30-day"},
            {"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}"#;
        let b = parse_daily_stats(value_type).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        // … but a real `value` key after a `valueType` still parses.
        let both = r#"{"selectedDate":"2026-08-19","pcRatios":[
            {"name":"TOTAL PUT/CALL RATIO","valueType":"pc","value":"0.80"}]}"#;
        let b = parse_daily_stats(both).unwrap();
        assert_eq!(b.total, Some(0.80), "{b:?}");
        // And a null / drifted selectedDate must not borrow a nearby date.
        let null_date = r#"{"selectedDate":null,"minDate":"2019-10-07",
            "pcRatios":[{"name":"TOTAL PUT/CALL RATIO","value":"0.80"}]}"#;
        assert!(parse_daily_stats(null_date).is_err());
    }

    #[test]
    fn extended_labels_nested_values_and_partial_numbers_never_serve_data() {
        // A longer product label ("… 30-DAY") is a different ratio — the
        // label's own closing quote must follow immediately (Codex round 6).
        let extended = r#"{"selectedDate":"2026-08-19","pcRatios":[{"name":"TOTAL PUT/CALL RATIO 30-DAY","value":"0.99"},{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}"#;
        let b = parse_daily_stats(extended).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // A `value` inside a NESTED object never anchors; the row's own value
        // is null → a gap, never the nested metadata's number.
        let nested = r#"{"selectedDate":"2026-08-19","pcRatios":[{"name":"TOTAL PUT/CALL RATIO","meta":{"value":"0.99"},"value":null},{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}"#;
        let b = parse_daily_stats(nested).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // The number must be the WHOLE scalar: "1e2" and "0.80oops" are
        // drift, never 1.0 / 0.80.
        for bad in [r#""1e2""#, r#""0.80oops""#] {
            let body = format!(
                r#"{{"selectedDate":"2026-08-19","pcRatios":[{{"name":"TOTAL PUT/CALL RATIO","value":{bad}}},{{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}}]}}"#
            );
            let b = parse_daily_stats(&body).unwrap();
            assert_eq!(b.total, None, "{bad}: {b:?}");
            assert_eq!(b.equity, Some(0.52));
        }
        // And a datetime under the exact selectedDate key is a changed
        // schema — drift, not an as-of.
        let datetime = r#"{"selectedDate":"2026-08-19T16:00","pcRatios":[{"name":"TOTAL PUT/CALL RATIO","value":"0.80"}]}"#;
        assert!(parse_daily_stats(datetime).is_err());
    }

    #[test]
    fn braces_inside_strings_and_loose_closes_never_serve_data() {
        // A `}` INSIDE a string value deflated the old synthetic brace
        // depth, letting a nested object's `value` read back at apparent top
        // level — 0.99 served while the row's own value is null (Codex round
        // 7). Brace-free anchoring makes the nested read structurally
        // unreachable: a gap, never a fabricated ratio.
        let brace_in_string = r#"{"selectedDate":"2026-08-19","pcRatios":[{"name":"TOTAL PUT/CALL RATIO","meta":{"note":"}","value":"0.99"},"value":null},{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}"#;
        let b = parse_daily_stats(brace_in_string).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // An escape CONTINUES the label — backslash-u0020 is a space inside
        // the string, so this is the 30-DAY product again, never a close
        // (round 7; the backslash is spelled \x5C so the escape reaches the
        // fixture literally).
        let bs = '\x5C';
        let escaped_label = format!(
            r#"{{"selectedDate":"2026-08-19","pcRatios":[{{"name":"TOTAL PUT/CALL RATIO{bs}u002030-DAY","value":"0.99"}},{{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}}]}}"#
        );
        let b = parse_daily_stats(&escaped_label).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // In-string whitespace is not a close: "0.80 percent" is a drifted
        // scalar, never 0.80.
        let in_string_space = r#"{"selectedDate":"2026-08-19","pcRatios":[{"name":"TOTAL PUT/CALL RATIO","value":"0.80 percent"},{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}"#;
        let b = parse_daily_stats(in_string_space).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // A quoted number cut by the bounded window's edge is truncation,
        // not a value — the real scalar is 0.809999, and reading 0.80 off
        // the cut would fabricate a ratio. The padding sizes the 60-byte
        // window to end exactly after "0.80".
        let truncated = format!(
            r#"{{"selectedDate":"2026-08-19","pcRatios":[{{"name":"TOTAL PUT/CALL RATIO","pad":"{}","value":"0.809999"}},{{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}}]}}"#,
            "x".repeat(36)
        );
        let b = parse_daily_stats(&truncated).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // A bare JSON number still parses — its close is the row's own
        // structural punctuation, visible past the anchoring head.
        let bare = r#"{"selectedDate":"2026-08-19","pcRatios":[{"name":"TOTAL PUT/CALL RATIO","value":0.80},{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}"#;
        let b = parse_daily_stats(bare).unwrap();
        assert_eq!(b.total, Some(0.80), "{b:?}");
        // And an escape continuing the DATE scalar is drift, not an as-of.
        let escaped_date = format!(
            r#"{{"selectedDate":"2026-08-19{bs}u2026","pcRatios":[{{"name":"TOTAL PUT/CALL RATIO","value":"0.80"}}]}}"#
        );
        assert!(parse_daily_stats(&escaped_date).is_err());
    }

    #[test]
    fn plain_form_literal_quotes_are_content_never_boundaries() {
        // In plain JSON a backslash-quote is a literal quote INSIDE the
        // string (Codex round 8) — the escaped payload's delimiter, but this
        // form's content. Three shapes that previously mis-read it as a
        // boundary (the backslash is spelled \x5C so it reaches the fixtures
        // literally):
        let bs = '\x5C';
        // … a scalar continuing past a literal quote is drift, never 0.80 …
        let in_scalar = format!(
            r#"{{"selectedDate":"2026-08-19","pcRatios":[{{"name":"TOTAL PUT/CALL RATIO","value":"0.80{bs}" percent"}},{{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}}]}}"#
        );
        let b = parse_daily_stats(&in_scalar).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // … a label continuing past one is a longer product's name …
        let in_label = format!(
            r#"{{"selectedDate":"2026-08-19","pcRatios":[{{"name":"TOTAL PUT/CALL RATIO{bs}" 30-DAY","value":"0.99"}},{{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}}]}}"#
        );
        let b = parse_daily_stats(&in_label).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // … and a date continuing past one is drift, not an as-of.
        let in_date = format!(
            r#"{{"selectedDate":"2026-08-19{bs}"junk","pcRatios":[{{"name":"TOTAL PUT/CALL RATIO","value":"0.80"}}]}}"#
        );
        assert!(parse_daily_stats(&in_date).is_err());
        // A key-shaped text INSIDE a plain string never anchors: its quotes
        // are escaped literals — the wrong form for a plain-opened row — so
        // the row's own null value stays a gap, never 0.99.
        let fake_key = format!(
            r#"{{"selectedDate":"2026-08-19","pcRatios":[{{"name":"TOTAL PUT/CALL RATIO","note":"see {bs}"value{bs}": 0.99 here","value":null}},{{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}}]}}"#
        );
        let b = parse_daily_stats(&fake_key).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
    }

    #[test]
    fn the_backdrop_reads_one_representation_anchored_by_the_date() {
        let bs = '\x5C';
        // An escaped row-shaped text quoted inside a PLAIN document is
        // internally consistent, but it is the wrong form for the plain
        // date that anchors the record (Codex round 9) — the real plain
        // equity row parses, the stringified fake never joins.
        let escaped_fake_row = format!(
            r#"{{"selectedDate":"2026-08-19","note":"{bs}"name{bs}":{bs}"TOTAL PUT/CALL RATIO{bs}",{bs}"value{bs}":{bs}"0.99{bs}"","pcRatios":[{{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}}]}}"#
        );
        let b = parse_daily_stats(&escaped_fake_row).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // The other direction: an escaped selectedDate quoted inside a
        // plain string classifies the document Escaped, so the real plain
        // rows are the wrong form — no ratio parses and the whole feed is a
        // typed gap, never a backdrop dated by an in-string fake.
        let escaped_fake_date = format!(
            r#"{{"note":"{bs}"selectedDate{bs}":{bs}"2026-08-19{bs}"","pcRatios":[{{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}}]}}"#
        );
        assert!(parse_daily_stats(&escaped_fake_date).is_err());
    }

    #[test]
    fn conflicting_same_form_candidates_are_drift_never_first_match_wins() {
        // A same-form decoy object beside the real collection (Codex round
        // 10): the decoy's 0.99 and the real row's null disagree, and the
        // chunk-split scanner has no structural way to tell which container
        // is operative — so the label is a gap, never the decoy's number.
        let decoy_row = r#"{"sample":{"name":"TOTAL PUT/CALL RATIO","value":"0.99"},"optionsData":{"selectedDate":"2026-08-19","ratios":[{"name":"TOTAL PUT/CALL RATIO","value":null},{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}}"#;
        let b = parse_daily_stats(decoy_row).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // Two same-form selectedDate keys that disagree leave no honest
        // as-of: the whole feed is a typed gap — never the earlier key's
        // date over the collection's own.
        let dual_dates = r#"{"filters":{"selectedDate":"2025-01-01"},"optionsData":{"selectedDate":"2026-08-19","ratios":[{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}}"#;
        assert!(parse_daily_stats(dual_dates).is_err());
        // Agreement is not ambiguity: duplicated identical rows and dates
        // (a re-flushed chunk) still serve their one value.
        let duplicated = r#"{"selectedDate":"2026-08-19","a":[{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}],"b":{"selectedDate":"2026-08-19","rows":[{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}}"#;
        let b = parse_daily_stats(duplicated).unwrap();
        assert_eq!(b.equity, Some(0.52), "{b:?}");
        // And two anchored rows carrying different numbers for one label
        // are a gap — here the only label, so the parse itself is drift.
        let conflicting = r#"{"selectedDate":"2026-08-19","rows":[{"name":"EQUITY PUT/CALL RATIO","value":"0.52"},{"name":"EQUITY PUT/CALL RATIO","value":"0.53"}]}"#;
        assert!(parse_daily_stats(conflicting).is_err());
    }

    #[test]
    fn prefix_extended_keys_and_unbound_labels_never_serve_data() {
        // A prefix-extended value key is a different key — `"value":null`
        // must not fall through to `"fallbackvalue"` (Codex round 5).
        let prefixed = r#"{"selectedDate":"2026-08-19","pcRatios":[{"name":"TOTAL PUT/CALL RATIO","value":null,"fallbackvalue":"0.80"},{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}"#;
        let b = parse_daily_stats(prefixed).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // A prefix-extended date key never serves the as-of.
        let prefixed_date = r#"{"fallbackselectedDate":"2026-08-19","pcRatios":[{"name":"TOTAL PUT/CALL RATIO","value":"0.80"}]}"#;
        assert!(parse_daily_stats(prefixed_date).is_err());
        // A label appearing as an unrelated field's text is not a ratio row —
        // only a `"name"` key's own value anchors a read.
        let unbound = r#"{"selectedDate":"2026-08-19","pcRatios":[{"note":"TOTAL PUT/CALL RATIO","value":"0.99"},{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}"#;
        let b = parse_daily_stats(unbound).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
    }

    #[test]
    fn a_valueless_row_never_reads_the_next_rows_value_and_partial_keys_never_anchor() {
        // COMPACT row boundary (no whitespace padding — the production shape):
        // a valueless total row sits immediately before the equity row, whose
        // value lands inside the 60-byte window; the row-closing brace must
        // stop the scan (Codex 2026-08-20 round 4).
        let compact = r#"{"selectedDate":"2026-08-19","pcRatios":[{"name":"TOTAL PUT/CALL RATIO"},{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}"#;
        let b = parse_daily_stats(compact).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // The same shape in the escaped-payload form.
        let escaped = r#"self.__next_f.push([1,"{\"selectedDate\":\"2026-08-19\",\"pcRatios\":[{\"name\":\"TOTAL PUT/CALL RATIO\"},{\"name\":\"EQUITY PUT/CALL RATIO\",\"value\":\"0.52\"}]}"])"#;
        let b = parse_daily_stats(escaped).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // A longer key beginning with `value` is a different key, never an
        // anchor — its own digits must not read as the ratio.
        let value30 = r#"{"selectedDate":"2026-08-19","pcRatios":[{"name":"TOTAL PUT/CALL RATIO","value30":1},{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}"#;
        let b = parse_daily_stats(value30).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
        // And a longer date key never serves the as-of.
        let date_time = r#"{"selectedDateTime":"2026-08-19T16:00","pcRatios":[{"name":"TOTAL PUT/CALL RATIO","value":"0.80"}]}"#;
        assert!(parse_daily_stats(date_time).is_err());
    }

    #[test]
    fn ratios_anchor_on_the_value_key_never_the_first_number() {
        // A drifted row interposing another numeric field must not serve a
        // fabricated ratio: the value key anchors the read.
        let drifted = r#"{"selectedDate":"2026-08-19","pcRatios":[
            {"name":"TOTAL PUT/CALL RATIO","rank":1,"value":"0.80"}]}"#;
        let b = parse_daily_stats(drifted).unwrap();
        assert_eq!(b.total, Some(0.80), "{b:?}");
        // A row with no value key inside the window reads as a gap for that
        // ratio, not a number borrowed from elsewhere.
        let valueless = r#"{"selectedDate":"2026-08-19",
            "note":"TOTAL PUT/CALL RATIO pending","other":7,
            "pcRatios":[{"name":"EQUITY PUT/CALL RATIO","value":"0.52"}]}"#;
        let b = parse_daily_stats(valueless).unwrap();
        assert_eq!(b.total, None, "{b:?}");
        assert_eq!(b.equity, Some(0.52));
    }

    /// Live extraction smoke against the real daily page — run manually:
    /// `cargo test cboe_daily_smoke -- --ignored --nocapture`. This is where a
    /// Cboe page redesign (the embedded payload's labels or date key moving)
    /// surfaces.
    #[test]
    #[ignore = "hits the live cboe.com daily-statistics page"]
    fn cboe_daily_smoke() {
        let b = CboeDataSource::new().unwrap().put_call_backdrop().unwrap();
        eprintln!("CBOE backdrop: {b:?}");
        assert!(!b.as_of.is_empty());
        assert!(
            b.total.or(b.index).or(b.equity).is_some(),
            "at least one ratio parsed: {b:?}"
        );
    }

    #[test]
    fn wire_round_trip_and_non_2xx_error() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: DAILY_PAGE,
        }]);
        let b = CboeDataSource::new()
            .unwrap()
            .with_url(&server.base_url)
            .put_call_backdrop()
            .unwrap();
        assert_eq!(b.equity, Some(0.52));

        let server = MockHttp::serve(vec![Canned::Reply {
            status: 503,
            headers: vec![],
            body: "down",
        }]);
        assert!(CboeDataSource::new()
            .unwrap()
            .with_url(&server.base_url)
            .put_call_backdrop()
            .is_err());
    }
}
