//! SEC EDGAR — a keyless primary source for company financials
//! (`docs/data-sources.md §SEC EDGAR`), used by the local Portfolio Analysis job
//! alongside FMP. This slice reads the **XBRL company-facts** API
//! (`/api/xbrl/companyfacts/CIK##########.json`), pulling the latest annual values
//! for a handful of GAAP concepts so the financial-analysis engine can cross-check
//! and fill gaps the FMP per-company pull leaves.
//!
//! Like the gated adapters it carries a base-URL injection seam so a localhost mock
//! exercises the full URL-build → retry → parse → domain-output wire path offline
//! (`crate::test_http`). It is **keyless** (like BLS/CFTC) — the only requirement is
//! a descriptive `User-Agent`, which SEC asks all automated clients to send. Failures
//! are fail-soft: a concept that can't be resolved is a `None`, not a fabricated
//! level, mirroring the data-honesty stance of every other adapter.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::progress::RunContext;

/// SEC EDGAR data host. The company-facts path is joined onto this.
const SEC_DATA_BASE: &str = "https://data.sec.gov";

/// SEC asks automated clients to identify themselves with a descriptive User-Agent
/// (a generic browser UA gets throttled). Static, since this is an app-level client.
pub(crate) const SEC_USER_AGENT: &str = "MarketSignal local-analysis (support@market-signal.app)";

/// The company-facts endpoint path; `{cik}` is the 10-digit zero-padded CIK.
fn company_facts_path(cik10: &str) -> String {
    format!("/api/xbrl/companyfacts/CIK{cik10}.json")
}

/// The submissions endpoint serving a filer's recent-filings index — the quick
/// check's EDGAR filing sweep (`docs/portfolio-analysis.md` §The quick check).
fn submissions_path(cik10: &str) -> String {
    format!("/submissions/CIK{cik10}.json")
}

/// Conservative, transient earnings-release recovery. These are discovery
/// parsers, not the forensic filing classifier or a new financial-data feed.
pub(crate) mod earnings {
    use anyhow::{bail, Context, Result};
    use chrono::NaiveDate;
    use reqwest::Url;
    use serde_json::Value;

    pub const MAX_CANDIDATES: usize = 4;
    pub const MAX_ATTEMPTS: u32 = 10;

    #[derive(Debug, Clone)]
    pub struct Issuer {
        pub symbol: String,
        pub cik: String,
        corporate_host: String,
    }

    impl Issuer {
        pub fn new(symbol: &str, cik: &str, website: &str) -> Option<Self> {
            if cik.len() != 10
                || !cik.bytes().all(|b| b.is_ascii_digit())
                || cik.parse::<u64>().ok()? == 0
            {
                return None;
            }
            let url = Url::parse(website).ok()?;
            crate::web_research::fetch::check_url_policy(website).ok()?;
            if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
                return None;
            }
            let host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
            let corporate_host = host.strip_prefix("www.").unwrap_or(&host).to_string();
            if corporate_host.parse::<std::net::IpAddr>().is_ok() || !corporate_host.contains('.') {
                return None;
            }
            Some(Self {
                symbol: symbol.to_ascii_uppercase(),
                cik: cik.to_string(),
                corporate_host,
            })
        }

        pub fn allows(&self, address: &str) -> bool {
            let Ok(url) = Url::parse(address) else {
                return false;
            };
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.port().is_some()
            {
                return false;
            }
            let host = url
                .host_str()
                .unwrap_or_default()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            host == self.corporate_host
                || ["www", "ir", "investor", "investors"]
                    .iter()
                    .any(|label| host == format!("{label}.{}", self.corporate_host))
        }

        pub fn submissions_url(&self) -> String {
            format!("https://data.sec.gov{}", super::submissions_path(&self.cik))
        }

        fn archive_root(&self) -> String {
            format!(
                "https://www.sec.gov/Archives/edgar/data/{}/",
                self.cik.trim_start_matches('0')
            )
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ReleaseTarget {
        pub year: i32,
        pub quarter: u32,
        pub fiscal: bool,
    }

    fn words(text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_ascii_lowercase)
            .collect()
    }

    fn quarters(tokens: &[String]) -> std::collections::BTreeSet<u32> {
        let mut out = std::collections::BTreeSet::new();
        for (i, token) in tokens.iter().enumerate() {
            let q = match token.as_str() {
                "q1" | "1q" => Some(1),
                "q2" | "2q" => Some(2),
                "q3" | "3q" => Some(3),
                "q4" | "4q" => Some(4),
                "first" | "1st" | "1" if tokens.get(i + 1).is_some_and(|v| v == "quarter") => {
                    Some(1)
                }
                "second" | "2nd" | "2" if tokens.get(i + 1).is_some_and(|v| v == "quarter") => {
                    Some(2)
                }
                "third" | "3rd" | "3" if tokens.get(i + 1).is_some_and(|v| v == "quarter") => {
                    Some(3)
                }
                "fourth" | "4th" | "4" if tokens.get(i + 1).is_some_and(|v| v == "quarter") => {
                    Some(4)
                }
                _ => None,
            };
            if let Some(q) = q {
                out.insert(q);
            }
        }
        out
    }

    fn years(tokens: &[String]) -> std::collections::BTreeSet<i32> {
        tokens
            .iter()
            .filter_map(|w| {
                let w = w.strip_prefix("fy").unwrap_or(w);
                (w.len() == 4)
                    .then(|| w.parse::<i32>().ok())
                    .flatten()
                    .filter(|y| (1900..=2100).contains(y))
            })
            .collect()
    }

    fn requested_period(text: &str) -> Option<ReleaseTarget> {
        let tokens = words(text);
        let quarters = quarters(&tokens);
        let years = years(&tokens);
        if quarters.len() != 1 || years.len() != 1 {
            return None;
        }
        Some(ReleaseTarget {
            year: *years.first()?,
            quarter: *quarters.first()?,
            fiscal: tokens
                .iter()
                .any(|w| w == "fiscal" || w == "fy" || w.starts_with("fy20")),
        })
    }

    // Match compact, literal fiscal labels rather than assembling a period
    // from arbitrary nearby words (for example results plus next-year guidance).
    fn fiscal_period(tokens: &[String], start: usize) -> Option<(usize, i32, u32)> {
        fn quarter(tokens: &[String], start: usize) -> Option<(usize, u32)> {
            for len in [1, 2] {
                let slice = tokens.get(start..start + len)?;
                if let Some(q) = quarters(slice).first() {
                    return Some((start + len, *q));
                }
            }
            None
        }
        fn year(tokens: &[String], start: usize) -> Option<(usize, i32)> {
            let token = tokens.get(start)?;
            let (end, value) = if matches!(token.as_str(), "fiscal" | "fy") {
                let next =
                    start + 1 + usize::from(tokens.get(start + 1).is_some_and(|w| w == "year"));
                (next + 1, tokens.get(next)?.as_str())
            } else {
                (start + 1, token.strip_prefix("fy")?)
            };
            let year = value.parse::<i32>().ok()?;
            (value.len() == 4 && (1900..=2100).contains(&year)).then_some((end, year))
        }
        if let Some((mut next, q)) = quarter(tokens, start) {
            if tokens.get(next).is_some_and(|w| w == "of") {
                next += 1;
            }
            if tokens.get(next).is_some_and(|w| w == "the") {
                next += 1;
            }
            let (end, y) = year(tokens, next)?;
            Some((end, y, q))
        } else {
            let (mut next, y) = year(tokens, start)?;
            if tokens.get(next).is_some_and(|w| w == "the") {
                next += 1;
            }
            let (end, q) = quarter(tokens, next)?;
            Some((end, y, q))
        }
    }

    fn announcement_date(text: &str) -> bool {
        let tokens = words(text);
        let date = tokens.iter().enumerate().any(|(i, word)| {
            matches!(word.as_str(), "date" | "dates")
                && !(word == "date" && i >= 2 && tokens[i - 2..i] == ["year", "to"])
        });
        date && tokens.iter().any(|word| {
            matches!(
                word.as_str(),
                "announce"
                    | "announces"
                    | "announced"
                    | "announcement"
                    | "set"
                    | "sets"
                    | "schedule"
                    | "schedules"
                    | "scheduled"
                    | "scheduling"
                    | "release"
                    | "publication"
                    | "reporting"
            )
        })
    }

    impl ReleaseTarget {
        pub fn from_request(url: &str, title: Option<&str>) -> Option<Self> {
            let parsed = Url::parse(url).ok()?;
            let path = parsed.path();
            let title = title.unwrap_or_default();
            if path.len() > 2048 || title.len() > 1024 {
                return None;
            }
            let words = words(&format!("{path} {title}"));
            if !words.iter().any(|w| w == "results")
                || words.iter().any(|w| {
                    matches!(
                        w.as_str(),
                        "consensus"
                            | "production"
                            | "deliveries"
                            | "webcast"
                            | "preliminary"
                            | "restated"
                            | "revised"
                            | "corrected"
                            | "correction"
                            | "amended"
                    )
                })
                || announcement_date(path)
                || announcement_date(title)
                || words
                    .windows(2)
                    .any(|p| matches!(p[0].as_str(), "to" | "will") && p[1] == "announce")
            {
                return None;
            }
            // Missing information may come from the other lead; conflicting
            // information may not. Check partial and internally ambiguous leads
            // together before the Option-based complete-period fallback.
            if quarters(&words).len() > 1 || years(&words).len() > 1 {
                return None;
            }
            // Title and URL are leads, not evidence. Conflicting explicit
            // periods are unresolved; a year in a URL is never shifted to Q4
            // of the prior year to make a candidate fit.
            match (requested_period(path), requested_period(title)) {
                (Some(a), Some(b)) if a != b => None,
                (Some(a), _) | (_, Some(a)) => Some(a),
                _ => None,
            }
        }

        pub fn label(&self) -> String {
            format!(
                "{}Q{} {}",
                if self.fiscal { "fiscal " } else { "" },
                self.quarter,
                self.year
            )
        }

        fn calendar_end(&self) -> NaiveDate {
            let (month, day) = match self.quarter {
                1 => (3, 31),
                2 => (6, 30),
                3 => (9, 30),
                _ => (12, 31),
            };
            NaiveDate::from_ymd_opt(self.year, month, day).unwrap()
        }

        pub fn matches_filing(&self, text: &str, calendar_issuer: bool) -> bool {
            let tokens = words(text);
            // An explicit source fiscal label is matched literally, never
            // converted into a calendar quarter from a filing/event date.
            if self.fiscal {
                // This bounded parser does not resolve actual-versus-projected
                // fiscal results within a mixed guidance section.
                if tokens.iter().any(|w| {
                    matches!(
                        w.as_str(),
                        "guidance"
                            | "outlook"
                            | "forecast"
                            | "forecasts"
                            | "expected"
                            | "projected"
                            | "projection"
                            | "projections"
                    )
                }) {
                    return false;
                }
                let mut periods = std::collections::BTreeSet::new();
                let mut results_match = false;
                for start in 0..tokens.len() {
                    if let Some((end, year, quarter)) = fiscal_period(&tokens, start) {
                        periods.insert((year, quarter));
                        let before = &tokens[..start];
                        let after = &tokens[end..];
                        let preceding_results = before
                            .iter()
                            .rposition(|w| w == "results")
                            .is_some_and(|i| {
                                before[i + 1..]
                                    .iter()
                                    .all(|w| matches!(w.as_str(), "for" | "of" | "the" | "our"))
                            });
                        let following_results = after.first().is_some_and(|w| w == "results")
                            || (after
                                .first()
                                .is_some_and(|w| matches!(w.as_str(), "financial" | "operating"))
                                && after.get(1).is_some_and(|w| w == "results"));
                        results_match |= (preceding_results || following_results)
                            && year == self.year
                            && quarter == self.quarter;
                    }
                }
                return results_match
                    && periods.len() == 1
                    && quarters(&tokens) == std::collections::BTreeSet::from([self.quarter]);
            }
            if !calendar_issuer {
                return false;
            }
            let end = self.calendar_end();
            let periods: std::collections::BTreeSet<_> = tokens
                .windows(5)
                .filter_map(|w| {
                    (w[0] == "quarter" && w[1] == "ended")
                        .then(|| date_from_words(&w[2..5]))
                        .flatten()
                })
                .collect();
            periods.len() == 1 && periods.contains(&end)
        }
    }

    fn date_from_words(w: &[String]) -> Option<NaiveDate> {
        let month = [
            "january",
            "february",
            "march",
            "april",
            "may",
            "june",
            "july",
            "august",
            "september",
            "october",
            "november",
            "december",
        ]
        .iter()
        .position(|m| w.first().is_some_and(|v| v == m))? as u32
            + 1;
        NaiveDate::from_ymd_opt(w.get(2)?.parse().ok()?, month, w.get(1)?.parse().ok()?)
    }

    pub fn publication_date(text: &str) -> Option<String> {
        // Only the release's own opening dateline. Filing/report dates and
        // search-result dates are never relabeled as publication provenance.
        let opening = text.chars().take(1800).collect::<String>();
        // Require an actual dateline: CITY, Month D, YYYY followed by a dash.
        // A quarter-end date printed earlier in the release is not publication.
        for before_dash in opening.split(['–', '—']) {
            let parts: Vec<_> = before_dash.rsplitn(3, ',').collect();
            if parts.len() != 3 {
                continue;
            }
            let year = parts[0].trim();
            if year.len() != 4 || !year.bytes().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let city = parts[2].split_whitespace().last().unwrap_or_default();
            if city.is_empty() || !city.chars().all(|c| c.is_ascii_uppercase()) {
                continue;
            }
            let date = format!("{} {year}", parts[1].trim());
            if let Some(date) = date_from_words(&words(&date)) {
                return Some(date.to_string());
            }
        }
        None
    }

    #[derive(Debug, Clone)]
    pub struct Candidate {
        pub index_url: String,
        pub primary_url: String,
        pub amended: bool,
    }

    #[derive(Debug)]
    pub struct Candidates {
        pub rows: Vec<Candidate>,
        pub calendar_issuer: bool,
        pub name: String,
    }

    /// Validate the requested issuer against the response itself. Candidate
    /// dates bound discovery only; actual period matching uses filing text.
    pub fn candidates(
        body: &str,
        issuer: &Issuer,
        target: &ReleaseTarget,
        reported_publication: Option<&str>,
    ) -> Result<Candidates> {
        let value: Value = serde_json::from_str(body).context("SEC submissions JSON")?;
        let cik = value["cik"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| value["cik"].as_u64().map(|n| n.to_string()));
        if cik.as_deref().and_then(|s| s.parse::<u64>().ok()) != issuer.cik.parse::<u64>().ok()
            || !value["tickers"]
                .as_array()
                .is_some_and(|a| a.iter().any(|t| t.as_str() == Some(&issuer.symbol)))
        {
            bail!("SEC submissions issuer identity mismatch");
        }
        let name = value["name"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .context("SEC issuer name absent")?
            .to_string();
        let calendar_issuer = value["fiscalYearEnd"].as_str() == Some("1231");
        let reported = reported_publication
            .and_then(|s| s.get(..10))
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
        // The explicit dated lead is a search constraint, never publication
        // evidence. Without it, only a verified calendar-year issuer can use
        // the quarter-end through following-quarter reporting window.
        let (from, through) = if let Some(date) = reported {
            (
                date - chrono::Duration::days(7),
                date + chrono::Duration::days(7),
            )
        } else if calendar_issuer && !target.fiscal {
            let end = target.calendar_end();
            (end, end + chrono::Duration::days(100))
        } else {
            bail!("release date required for non-calendar or fiscal-label recovery");
        };
        let recent = &value["filings"]["recent"];
        let forms = recent["form"].as_array().context("SEC forms absent")?;
        for key in [
            "accessionNumber",
            "filingDate",
            "reportDate",
            "items",
            "primaryDocument",
        ] {
            if recent[key]
                .as_array()
                .is_none_or(|a| a.len() != forms.len())
            {
                bail!("SEC unpaired {key} column");
            }
        }
        let mut rows = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (i, form) in forms.iter().enumerate() {
            let form = form.as_str().context("SEC unreadable form")?;
            if !matches!(form, "8-K" | "8-K/A") {
                continue;
            }
            let date = |key: &str| -> Result<NaiveDate> {
                Ok(NaiveDate::parse_from_str(
                    recent[key][i].as_str().context("SEC missing filing date")?,
                    "%Y-%m-%d",
                )?)
            };
            let filed = date("filingDate")?;
            let report = date("reportDate")?;
            // Later amendments remain plausible even outside the reporting
            // window; they must be inspected or make the bounded read unresolved.
            if form == "8-K/A" {
                if filed < from {
                    continue;
                }
            } else if !((from..=through).contains(&filed) || (from..=through).contains(&report)) {
                continue;
            }
            let items = recent["items"][i]
                .as_str()
                .context("SEC unreadable items")?;
            if form != "8-K/A" && !items.split(',').any(|s| s.trim() == "2.02") {
                continue;
            }
            let accession = recent["accessionNumber"][i]
                .as_str()
                .context("SEC accession absent")?;
            if accession.len() != 20
                || accession.as_bytes()[10] != b'-'
                || accession.as_bytes()[13] != b'-'
                || !accession
                    .replace('-', "")
                    .bytes()
                    .all(|c| c.is_ascii_digit())
            {
                bail!("SEC malformed accession");
            }
            let primary = recent["primaryDocument"][i]
                .as_str()
                .context("SEC primary document absent")?;
            if primary.is_empty()
                || !primary
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
                || primary.contains("..")
            {
                bail!("SEC unsafe primary document");
            }
            if !seen.insert(accession.to_string()) {
                continue;
            }
            let root = format!("{}{}/", issuer.archive_root(), accession.replace('-', ""));
            rows.push(Candidate {
                index_url: format!("{root}{accession}-index.htm"),
                primary_url: format!("{root}{primary}"),
                amended: form == "8-K/A",
            });
        }
        if rows.len() > MAX_CANDIDATES {
            bail!("more than {MAX_CANDIDATES} plausible SEC filings; match unresolved");
        }
        Ok(Candidates {
            rows,
            calendar_issuer,
            name,
        })
    }

    pub fn document_url_allowed(url: &Url) -> bool {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && match url.host_str() {
                Some("data.sec.gov") => {
                    url.path().starts_with("/submissions/CIK") && url.path().ends_with(".json")
                }
                Some("www.sec.gov") => url.path().starts_with("/Archives/edgar/data/"),
                _ => false,
            }
    }

    pub fn document_text(html: &str, url: &str) -> Result<String> {
        let parsed = dom_smoothie::Readability::new(html, Some(url), None)?;
        Ok(parsed.doc.text().to_string())
    }

    /// Read the filing-detail document table, never guess an exhibit filename
    /// or take the first directory entry named 99.1.
    pub fn exhibit_url(html: &str, candidate: &Candidate) -> Result<String> {
        let parsed = dom_smoothie::Readability::new(html, Some(&candidate.index_url), None)?;
        let base = Url::parse(&candidate.index_url)?;
        let root = base.join(".")?;
        let mut primary_seen = false;
        let mut exhibits = std::collections::BTreeSet::new();
        for row in parsed.doc.select("table.tableFile tr").iter() {
            let cells = row.select("td");
            let cells: Vec<_> = cells.iter().collect();
            if cells.len() < 4 {
                continue;
            }
            let kind = cells[3].text().trim().to_ascii_uppercase();
            if !matches!(kind.as_str(), "8-K" | "8-K/A" | "EX-99.1") {
                continue;
            }
            let href = cells[2]
                .select("a")
                .attr("href")
                .context("SEC document link absent")?;
            let mut link = base.join(&href)?;
            // SEC's filing index may point the primary inline-XBRL document
            // through its viewer. Unwrap only that exact official doc parameter.
            if link.host_str() == Some("www.sec.gov") && link.path() == "/ix" {
                let query: Vec<_> = link.query_pairs().collect();
                if query.len() != 1 || query[0].0 != "doc" {
                    bail!("unrecognized SEC viewer link");
                }
                link = base.join(&query[0].1)?;
            }
            if !document_url_allowed(&link) || !link.as_str().starts_with(root.as_str()) {
                bail!("SEC exhibit link leaves the selected issuer/accession");
            }
            if kind == "EX-99.1" {
                exhibits.insert(link.to_string());
            } else if link.as_str() == candidate.primary_url {
                primary_seen = true;
            }
        }
        if !primary_seen || exhibits.len() != 1 {
            bail!("SEC filing document table has no unique EX-99.1 relationship");
        }
        Ok(exhibits.into_iter().next().unwrap())
    }

    pub fn confirms_relationship(
        html: &str,
        candidate: &Candidate,
        exhibit: &str,
        target: &ReleaseTarget,
        calendar: bool,
    ) -> Result<bool> {
        let text = document_text(html, &candidate.primary_url)?;
        let tokens = words(&text);
        let Some(start) = tokens.windows(3).position(|w| w == ["item", "2", "02"]) else {
            return Ok(false);
        };
        let tail = &tokens[start + 3..];
        let end = tail.iter().position(|w| w == "item").unwrap_or(tail.len());
        let section = &tail[..end];
        let Some(first_exhibit) = section.iter().position(|w| w == "exhibit") else {
            return Ok(false);
        };
        let release = section
            .windows(2)
            .any(|w| w == ["press", "release"] || w == ["earnings", "release"]);
        let exhibit_991 = section.get(first_exhibit + 1).is_some_and(|w| w == "99")
            && section.get(first_exhibit + 2).is_some_and(|w| w == "1");
        let parsed = dom_smoothie::Readability::new(html, Some(&candidate.primary_url), None)?;
        let base = Url::parse(&candidate.primary_url)?;
        let linked = parsed.doc.select("a[href]").iter().any(|a| {
            a.attr("href")
                .and_then(|href| base.join(&href).ok())
                .is_some_and(|u| u.as_str() == exhibit)
        });
        let period_matches = target.matches_filing(&section.join(" "), calendar);
        if period_matches && has_release_revision(section) {
            bail!("preliminary or corrected earnings relationship unresolved");
        }
        Ok(release && exhibit_991 && linked && period_matches)
    }

    fn has_release_revision(tokens: &[String]) -> bool {
        tokens.iter().enumerate().any(|(i, word)| {
            if word == "amended" {
                // Exempt only named securities-law boilerplate, not arbitrary
                // 'as amended' text that could describe the release itself.
                let before = &tokens[..i];
                let legal = [
                    "securities act as",
                    "securities exchange act as",
                    "securities act of 1933 as",
                    "securities exchange act of 1934 as",
                    "securities act 1933 as",
                    "securities exchange act 1934 as",
                ]
                .iter()
                .any(|phrase| before.ends_with(&words(phrase)));
                !legal
            } else {
                matches!(
                    word.as_str(),
                    "preliminary" | "restated" | "revised" | "corrected" | "correction"
                )
            }
        })
    }

    pub fn usable_release(text: &str, name: &str, target: &ReleaseTarget) -> bool {
        let opening = words(&text.chars().take(1800).collect::<String>()).join(" ");
        let name = words(name)
            .into_iter()
            .filter(|w| {
                !matches!(
                    w.as_str(),
                    "inc" | "incorporated" | "corp" | "corporation" | "company" | "co" | "ltd"
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let quarter = quarters(&words(&opening));
        !text.trim().is_empty()
            && !name.is_empty()
            && opening.contains(&name)
            && quarter.contains(&target.quarter)
            && text.chars().any(|c| c == '$' || c == '%')
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        const SUBMISSIONS: &str =
            include_str!("portfolio/fixtures/edgar-recovery/psx-submissions.json");
        const INDEX: &str = include_str!("portfolio/fixtures/edgar-recovery/psx-index.htm");
        const FILING: &str = include_str!("portfolio/fixtures/edgar-recovery/psx-8k.htm");

        fn issuer() -> Issuer {
            Issuer::new("PSX", "0001534701", "https://www.phillips66.com").unwrap()
        }
        fn target() -> ReleaseTarget {
            ReleaseTarget {
                year: 2026,
                quarter: 2,
                fiscal: false,
            }
        }

        #[test]
        fn slice3_real_psx_filing_links_the_earnings_period_not_the_report_date() {
            let selected = candidates(SUBMISSIONS, &issuer(), &target(), None).unwrap();
            assert_eq!(selected.rows.len(), 1);
            let candidate = &selected.rows[0];
            let exhibit = exhibit_url(INDEX, candidate).unwrap();
            assert!(exhibit.ends_with("psx-2026630_ex991.htm"));
            assert!(confirms_relationship(
                FILING,
                candidate,
                &exhibit,
                &target(),
                selected.calendar_issuer
            )
            .unwrap());
            assert!(!confirms_relationship(
                FILING,
                candidate,
                &exhibit,
                &ReleaseTarget {
                    quarter: 3,
                    ..target()
                },
                true
            )
            .unwrap());
            assert!(!confirms_relationship(
                FILING,
                candidate,
                &exhibit,
                &ReleaseTarget {
                    year: 2025,
                    ..target()
                },
                true
            )
            .unwrap());
            // Source markup can split the phrase; the pure period matcher pins
            // the semantic boundary separately from this real DOM fixture.
            assert!(!target().matches_filing(
                "Report date August 5, 2026. Quarter ended June 30, 2025.",
                true
            ));
        }

        #[test]
        fn slice3_issuer_hosts_and_request_intent_fail_closed() {
            let issuer = issuer();
            for host in [
                "phillips66.com",
                "www.phillips66.com",
                "investor.phillips66.com",
                "IR.PHILLIPS66.COM.",
            ] {
                assert!(issuer.allows(&format!("https://{host}/news")));
            }
            for url in [
                "https://phillips66.com.evil.org/news",
                "https://evilphillips66.com/news",
                "https://ir.marathonpetroleum.com/news",
                "https://user@investor.phillips66.com/news",
                "https://investor.phillips66.com:444/news",
            ] {
                assert!(!issuer.allows(url), "{url}");
            }
            let url = "https://investor.phillips66.com/2026/second-quarter-results";
            assert_eq!(ReleaseTarget::from_request(url, None), Some(target()));
            assert!(ReleaseTarget::from_request(url, Some("First quarter 2026 results")).is_none());
            for path in [
                "2026/to-announce-second-quarter-results",
                "2026/second-quarter-delivery-consensus",
                "static-files/abcd",
                "2026/quarterly-results",
                "2026/full-year-results",
            ] {
                assert!(
                    ReleaseTarget::from_request(
                        &format!("https://investor.phillips66.com/{path}"),
                        None
                    )
                    .is_none(),
                    "{path}"
                );
            }
            let fiscal = ReleaseTarget::from_request(
                "https://example.com/results",
                Some("First quarter fiscal 2026 results"),
            )
            .unwrap();
            assert!(fiscal.fiscal);
            assert!(!fiscal.matches_filing(
                "First quarter 2026 results. Quarter ended March 31, 2026.",
                true
            ));
            assert!(fiscal.matches_filing(
                "We announced first quarter fiscal 2026 results on this date.",
                false
            ));
        }

        #[test]
        fn slice3_discovery_refuses_identity_drift_bad_links_and_uninspected_candidates() {
            let mut v: Value = serde_json::from_str(SUBMISSIONS).unwrap();
            v["tickers"] = serde_json::json!(["MPC"]);
            assert!(candidates(&v.to_string(), &issuer(), &target(), None).is_err());
            let mut v: Value = serde_json::from_str(SUBMISSIONS).unwrap();
            let recent = v["filings"]["recent"].as_object_mut().unwrap();
            for value in recent.values_mut() {
                let first = value[0].clone();
                *value = Value::Array(vec![first; 5]);
            }
            for i in 0..5 {
                recent.get_mut("accessionNumber").unwrap()[i] =
                    Value::String(format!("0001534701-26-{:06}", i + 30));
            }
            assert!(candidates(&v.to_string(), &issuer(), &target(), None)
                .unwrap_err()
                .to_string()
                .contains("plausible"));
            let candidate = candidates(SUBMISSIONS, &issuer(), &target(), None)
                .unwrap()
                .rows
                .remove(0);
            let bad = INDEX.replace(
                "/Archives/edgar/data/1534701/000153470126000030/psx-2026630_ex991.htm",
                "https://evil.example/exhibit.htm",
            );
            assert_ne!(bad, INDEX);
            assert!(exhibit_url(&bad, &candidate).is_err());
            assert!(exhibit_url(&INDEX.replace("EX-99.1", "EX-99.2"), &candidate).is_err());
            assert!(!document_url_allowed(
                &Url::parse("https://www.sec.gov.evil.org/Archives/edgar/data/1/a.htm").unwrap()
            ));
        }

        #[test]
        fn slice3_late_amendments_remain_plausible_and_fiscal_dates_are_not_inferred() {
            let mut v: Value = serde_json::from_str(SUBMISSIONS).unwrap();
            v["filings"]["recent"]["form"][0] = Value::String("8-K/A".into());
            v["filings"]["recent"]["filingDate"][0] = Value::String("2027-02-01".into());
            assert!(
                candidates(&v.to_string(), &issuer(), &target(), None)
                    .unwrap()
                    .rows[0]
                    .amended
            );
            v["fiscalYearEnd"] = Value::String("0630".into());
            assert!(candidates(&v.to_string(), &issuer(), &target(), None).is_err());
            assert!(candidates(&v.to_string(), &issuer(), &target(), Some("2026-08-05")).is_ok());
            assert_eq!(
                publication_date(
                    "Quarter ended June 30, 2026. HOUSTON, August 5, 2026 – Phillips 66 results"
                ),
                Some("2026-08-05".into())
            );
            assert_eq!(publication_date("Quarter ended June 30, 2026"), None);
        }

        #[test]
        fn slice3_period_and_exhibit_relationship_cannot_be_borrowed_from_another_item() {
            let candidate = candidates(SUBMISSIONS, &issuer(), &target(), None)
                .unwrap()
                .rows
                .remove(0);
            let exhibit = exhibit_url(INDEX, &candidate).unwrap();
            let html = |statement: &str| {
                format!(
                "<html><body><p>Item 2.02 Results of Operations and Financial Condition. {statement}</p><p>A copy of the press release is furnished as <a href='{exhibit}'>Exhibit 99.1</a>.</p><p>The Securities Exchange Act, as amended.</p><p>Item 9.01 Exhibits. Quarter ended June 30, 2026.</p></body></html>")
            };
            assert!(confirms_relationship(
                &html("We issued a press release for the quarter ended June 30, 2026."),
                &candidate,
                &exhibit,
                &target(),
                true
            )
            .unwrap());
            assert!(!confirms_relationship(
                &html("We issued a press release for the quarter ended March 31, 2026."),
                &candidate,
                &exhibit,
                &target(),
                true
            )
            .unwrap());
            assert!(!confirms_relationship(&html("We issued a press release for the quarter ended June 30, 2026 and the quarter ended March 31, 2026."), &candidate, &exhibit, &target(), true).unwrap());
            assert!(!confirms_relationship(&html("We issued a press release for the quarter ended June 30, 2026 in Exhibit 99.2."), &candidate, &exhibit, &target(), true).unwrap());
            assert!(confirms_relationship(
                &html("We issued a corrected press release for the quarter ended June 30, 2026."),
                &candidate,
                &exhibit,
                &target(),
                true
            )
            .is_err());
            let unrelated = format!("<html><body><p>Item 2.02 Dividends were announced.</p><p>Item 8.01 We issued a press release for the quarter ended June 30, 2026, attached as <a href='{exhibit}'>Exhibit 99.1</a>.</p></body></html>");
            assert!(
                !confirms_relationship(&unrelated, &candidate, &exhibit, &target(), true).unwrap()
            );
        }

        #[test]
        fn slice3_review_conflicting_request_leads_cannot_be_overridden() {
            for (path, title) in [
                (
                    "2026/first-quarter-and-second-quarter-results",
                    "Second quarter 2026 results",
                ),
                (
                    "2025/second-quarter-results-2026",
                    "Second quarter 2026 results",
                ),
                (
                    "2026/second-quarter-results",
                    "First quarter and second quarter 2026 results",
                ),
                (
                    "2026/second-quarter-results",
                    "Second quarter 2025 and 2026 results",
                ),
                ("2025/results", "Second quarter 2026 results"),
                ("first-quarter/results", "Second quarter 2026 results"),
            ] {
                assert!(
                    ReleaseTarget::from_request(
                        &format!("https://investor.phillips66.com/{path}"),
                        Some(title)
                    )
                    .is_none(),
                    "{path}: {title}"
                );
            }
            assert_eq!(
                ReleaseTarget::from_request(
                    "https://investor.phillips66.com/static-files/abcd",
                    Some("Second quarter 2026 results")
                ),
                Some(target())
            );
            assert_eq!(
                ReleaseTarget::from_request(
                    "https://investor.phillips66.com/2026/second-quarter-results",
                    None
                ),
                Some(target())
            );
        }

        #[test]
        fn slice3_review_announcement_dates_are_not_results_releases() {
            for notice in [
                "announces date for second quarter 2026 results",
                "sets date for second quarter 2026 results",
                "announced dates for second quarter 2026 results",
                "second quarter 2026 results announcement date",
                "will announce second quarter 2026 results",
                "to announce second quarter 2026 results",
            ] {
                assert!(
                    ReleaseTarget::from_request(
                        &format!(
                            "https://investor.phillips66.com/{}",
                            notice.replace(' ', "-")
                        ),
                        None
                    )
                    .is_none(),
                    "{notice}"
                );
                assert!(
                    ReleaseTarget::from_request(
                        "https://investor.phillips66.com/static-files/abcd",
                        Some(notice)
                    )
                    .is_none(),
                    "{notice}"
                );
            }
            for release in [
                "announces second quarter 2026 results",
                "reports second quarter 2026 results",
            ] {
                assert_eq!(
                    ReleaseTarget::from_request(
                        "https://investor.phillips66.com/news",
                        Some(release)
                    ),
                    Some(target())
                );
            }
        }

        #[test]
        fn slice3_review_year_to_date_results_remain_eligible_but_not_their_notices() {
            for release in [
                "second quarter and year-to-date 2026 results",
                "announces second quarter and year to date 2026 results",
                "reports second quarter 2026 and year-to-date results",
            ] {
                assert_eq!(
                    ReleaseTarget::from_request(
                        &format!(
                            "https://investor.phillips66.com/{}",
                            release.replace(' ', "-")
                        ),
                        None
                    ),
                    Some(target()),
                    "{release}"
                );
                assert_eq!(
                    ReleaseTarget::from_request(
                        "https://investor.phillips66.com/static-files/abcd",
                        Some(release)
                    ),
                    Some(target()),
                    "{release}"
                );
            }
            for notice in [
                "announces date for second quarter and year-to-date 2026 results",
                "sets date for second quarter and year-to-date 2026 results",
                "second quarter and year-to-date 2026 results announcement date",
                "scheduled release date for second quarter and year-to-date 2026 results",
            ] {
                assert!(
                    ReleaseTarget::from_request(
                        &format!(
                            "https://investor.phillips66.com/{}",
                            notice.replace(' ', "-")
                        ),
                        None
                    )
                    .is_none(),
                    "{notice}"
                );
                assert!(ReleaseTarget::from_request(
                    "https://investor.phillips66.com/2026/second-quarter-and-year-to-date-results", Some(notice)
                ).is_none(), "{notice}");
            }
        }

        #[test]
        fn slice3_review_corrections_after_exhibit_fail_closed_but_law_does_not() {
            let candidate = candidates(SUBMISSIONS, &issuer(), &target(), None)
                .unwrap()
                .rows
                .remove(0);
            let exhibit = exhibit_url(INDEX, &candidate).unwrap();
            for qualifier in ["corrected", "preliminary", "restated", "revised", "amended"] {
                let html = format!("<html><body><p>Item 2.02 We issued a press release, furnished as <a href='{exhibit}'>Exhibit 99.1</a>, containing {qualifier} results for the quarter ended June 30, 2026.</p><p>Item 9.01 Exhibits.</p></body></html>");
                assert!(
                    confirms_relationship(&html, &candidate, &exhibit, &target(), true).is_err(),
                    "{qualifier}"
                );
            }
            let html = format!("<html><body><p>Item 2.02 We issued a press release furnished as <a href='{exhibit}'>Exhibit 99.1</a> for the quarter ended June 30, 2026.</p><p>The release corrects earlier information: corrected financial results are attached.</p><p>Item 9.01 Exhibits.</p></body></html>");
            assert!(confirms_relationship(&html, &candidate, &exhibit, &target(), true).is_err());
            for law in [
                "Securities Act",
                "Securities Act of 1933",
                "Securities Exchange Act",
                "Securities Exchange Act of 1934",
            ] {
                let html = format!("<html><body><p>Item 2.02 Under the {law}, as amended, we issued a press release for the quarter ended June 30, 2026, furnished as <a href='{exhibit}'>Exhibit 99.1</a>.</p><p>The {law}, as amended.</p><p>Item 9.01 A corrected unrelated exhibit.</p></body></html>");
                assert!(
                    confirms_relationship(&html, &candidate, &exhibit, &target(), true).unwrap(),
                    "{law}"
                );
            }
        }

        #[test]
        fn slice3_review_fiscal_results_cannot_borrow_guidance_or_competing_periods() {
            let candidate = candidates(SUBMISSIONS, &issuer(), &target(), None)
                .unwrap()
                .rows
                .remove(0);
            let exhibit = exhibit_url(INDEX, &candidate).unwrap();
            let fiscal = ReleaseTarget {
                year: 2027,
                quarter: 1,
                fiscal: true,
            };
            let html = |statement: &str| {
                format!("<html><body><p>Item 2.02 Results of Operations and Financial Condition. We issued a press release with {statement}, furnished as <a href='{exhibit}'>Exhibit 99.1</a>.</p><p>Item 9.01 Exhibits.</p></body></html>")
            };
            for statement in [
                "fourth quarter fiscal 2026 results and first quarter fiscal 2027 guidance",
                "first quarter fiscal 2027 guidance and fourth quarter fiscal 2026 results",
                "first quarter fiscal 2026 results and first quarter fiscal 2027 guidance",
                "first quarter fiscal 2027 guidance",
                "guidance for first quarter fiscal 2027 results",
                "expected results for first quarter fiscal 2027",
                "first quarter fiscal 2027 results outlook",
                "guidance regarding financial and operating results for the first quarter fiscal 2027",
                "projected financial and operating results for the first quarter fiscal 2027",
                "first quarter fiscal 2027 results and second quarter fiscal 2027 results",
            ] {
                assert!(
                    !confirms_relationship(&html(statement), &candidate, &exhibit, &fiscal, false)
                        .unwrap(),
                    "{statement}"
                );
            }
            for statement in [
                "first quarter fiscal 2027 results",
                "results for the first quarter of fiscal year 2027",
                "fiscal 2027 first quarter results",
                "results for Q1 FY2027",
            ] {
                assert!(
                    confirms_relationship(&html(statement), &candidate, &exhibit, &fiscal, false)
                        .unwrap(),
                    "{statement}"
                );
                assert!(!confirms_relationship(
                    &html(statement),
                    &candidate,
                    &exhibit,
                    &ReleaseTarget {
                        year: 2026,
                        ..fiscal.clone()
                    },
                    false
                )
                .unwrap());
            }
        }
    }
}

/// The latest annual values pulled from a company's XBRL facts — each `None` when the
/// concept was not reported (or could not be resolved). Deliberately a small set: the
/// lines the engine cross-checks against FMP.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompanyFacts {
    pub revenue: Option<i64>,
    /// The prior fiscal year's revenue — read from the **same concept** that
    /// supplied `revenue` (never a different tag, so the growth read can't mix
    /// bases), the second-latest distinct annual period end. Feeds the annual-basis
    /// `revenue_growth` fallback where the FMP quarterly prints are too thin for
    /// the TTM basis (the grade-band slice's F5 closure).
    pub revenue_prior: Option<i64>,
    pub gross_profit: Option<i64>,
    pub net_income: Option<i64>,
    pub total_assets: Option<i64>,
    pub stockholders_equity: Option<i64>,
}

impl CompanyFacts {
    /// Whether any fact resolved — the dossier uses this to decide if SEC contributed.
    pub fn is_empty(&self) -> bool {
        self.revenue.is_none()
            && self.revenue_prior.is_none()
            && self.gross_profit.is_none()
            && self.net_income.is_none()
            && self.total_assets.is_none()
            && self.stockholders_equity.is_none()
    }
}

/// Where the full ticker → CIK map lives. It is served from the `www.sec.gov` host,
/// not the `data.sec.gov` API host the company-facts call uses, so it carries its own
/// base-URL seam.
const SEC_TICKERS_BASE: &str = "https://www.sec.gov";

/// The company-tickers file path on [`SEC_TICKERS_BASE`].
const SEC_TICKERS_PATH: &str = "/files/company_tickers.json";

/// How long a cached `company_tickers.json` stays fresh before a run refreshes it
/// (drafted — CIK assignments change rarely, so a week keeps the map current without
/// re-downloading the ~1 MB file per run). A stale cache is still used when the
/// refresh fetch fails: fail-soft, never a run blocker.
pub const CIK_CACHE_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The ticker → CIK resolver over SEC's full `company_tickers.json` map
/// (`docs/data-sources.md §SEC EDGAR`). Resolution returns the 10-digit zero-padded
/// CIK EDGAR expects; an unresolved ticker stays `None` and degrades to a typed gap
/// at the caller, never a fabricated mapping.
#[derive(Debug, Clone, Default)]
pub struct CikResolver {
    map: std::collections::HashMap<String, String>,
}

impl CikResolver {
    /// An empty resolver — every lookup misses. The fail-soft floor when neither a
    /// cache nor a fetch is available.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Parse the `company_tickers.json` body: an object keyed by row index, each row
    /// `{cik_str, ticker, title}`. The CIK is zero-padded to the 10 digits EDGAR paths
    /// expect.
    pub fn from_json(body: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(body).context("parsing company_tickers.json")?;
        let rows = value
            .as_object()
            .context("company_tickers.json: expected a top-level object")?;
        let mut map = std::collections::HashMap::with_capacity(rows.len());
        for row in rows.values() {
            let (Some(ticker), Some(cik)) = (
                row.get("ticker").and_then(Value::as_str),
                row.get("cik_str").and_then(Value::as_u64),
            ) else {
                continue; // A malformed row is skipped, never a fabricated mapping.
            };
            map.insert(ticker.to_ascii_uppercase(), format!("{cik:010}"));
        }
        Ok(Self { map })
    }

    /// The 10-digit zero-padded CIK for a ticker (case-insensitive), or `None` when
    /// the symbol has no EDGAR mapping.
    pub fn resolve(&self, ticker: &str) -> Option<&str> {
        self.map.get(&ticker.to_ascii_uppercase()).map(String::as_str)
    }

    /// How many tickers resolve — zero means the resolver is the empty fail-soft floor.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// The file name of the cached `company_tickers.json`, kept beside the app database.
const CIK_CACHE_FILE: &str = "sec_company_tickers.json";

/// Where the ticker → CIK cache lives for a given app database: the sibling
/// [`CIK_CACHE_FILE`] in the database's directory (the bare file name when the
/// path has no parent). Single-homed so both local jobs resolve against one cache.
pub fn cik_cache_path_beside(db_path: &std::path::Path) -> std::path::PathBuf {
    db_path
        .parent()
        .map(|d| d.join(CIK_CACHE_FILE))
        .unwrap_or_else(|| std::path::PathBuf::from(CIK_CACHE_FILE))
}

/// Load the ticker → CIK resolver: the on-disk cache when fresh
/// ([`CIK_CACHE_MAX_AGE`]), else a fetch that rewrites the cache. Fail-soft at every
/// step — a failed fetch falls back to a stale cache when one exists, and to the
/// empty resolver when none does, so an SEC outage degrades filings coverage to
/// typed gaps rather than blocking the run.
///
/// The refresh fetch honors the shared cancel flag like every SEC request
/// ([`SecEdgarSource::fetch_company_tickers`] bails without a request when it is
/// set), so it falls to the same stale-or-empty floor. That flag is only cleared
/// once a job owns the global run slot (`RunContext::reset_cancel`), which is why
/// the live jobs defer this load to first use inside the slot
/// ([`LazyCikResolver`]) rather than calling it eagerly at setup: an eager load
/// after a cancelled run would silently ship a stale or empty map into the whole
/// run, and its request row would fire before `run_started`.
pub fn load_cik_resolver(cache_path: &std::path::Path, source: &SecEdgarSource) -> CikResolver {
    let cached = std::fs::read_to_string(cache_path).ok();
    let cache_fresh = std::fs::metadata(cache_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|age| age < CIK_CACHE_MAX_AGE)
        .unwrap_or(false);
    if cache_fresh {
        if let Some(body) = &cached {
            if let Ok(resolver) = CikResolver::from_json(body) {
                return resolver;
            }
        }
    }
    match source.fetch_company_tickers() {
        Ok(body) => match CikResolver::from_json(&body) {
            Ok(resolver) => {
                // Best-effort cache write: a failed write costs the next run a
                // re-download, never this run's resolution.
                if let Some(dir) = cache_path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(cache_path, &body);
                resolver
            }
            Err(_) => stale_or_empty(cached),
        },
        Err(_) => stale_or_empty(cached),
    }
}

/// The fail-soft floor for [`load_cik_resolver`]: a parseable stale cache, else empty.
fn stale_or_empty(cached: Option<String>) -> CikResolver {
    cached
        .and_then(|body| CikResolver::from_json(&body).ok())
        .unwrap_or_else(CikResolver::empty)
}

/// The ticker → CIK resolver **deferred to first use** — the carrier of the local
/// jobs' ordering invariant: every external fetch happens inside the global run
/// slot, after `try_begin` + `reset_cancel` + `run_started`, so the ticker-map
/// refresh (a) sees the run's own cancel state rather than a prior cancelled run's
/// leftover flag, and (b) streams its request row under the active step instead
/// of firing before the tracker is listening. Constructing one performs no I/O;
/// the first [`Self::get`] runs [`load_cik_resolver`] once and memoizes the result
/// for the run (a fail-soft stale/empty map is memoized too — the same one-load-
/// per-run behavior the eager call had). The daemon probe stays the one pre-slot
/// check, and it is local-only.
pub struct LazyCikResolver {
    cache_path: std::path::PathBuf,
    resolver: std::sync::OnceLock<CikResolver>,
}

impl LazyCikResolver {
    /// Bind the cache location; nothing is read or fetched until [`Self::get`].
    pub fn new(cache_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            cache_path: cache_path.into(),
            resolver: std::sync::OnceLock::new(),
        }
    }

    /// A resolver that is already resolved — no cache, no fetch, ever. For a
    /// caller that holds a map (tests, offline smokes).
    pub fn preloaded(resolver: CikResolver) -> Self {
        let cell = std::sync::OnceLock::new();
        let _ = cell.set(resolver);
        Self {
            cache_path: std::path::PathBuf::from(CIK_CACHE_FILE),
            resolver: cell,
        }
    }

    /// The resolver, loading it through `source` on the first call (see
    /// [`load_cik_resolver`]) and serving the memoized map after that.
    pub fn get(&self, source: &SecEdgarSource) -> &CikResolver {
        self.resolver
            .get_or_init(|| load_cik_resolver(&self.cache_path, source))
    }

    /// Resolve one ticker, loading the map on first use — [`CikResolver::resolve`]
    /// behind the lazy load.
    pub fn resolve(&self, source: &SecEdgarSource, ticker: &str) -> Option<&str> {
        self.get(source).resolve(ticker)
    }

    /// Whether the map has been loaded yet — the ordering tests' probe.
    #[cfg(test)]
    pub fn is_loaded(&self) -> bool {
        self.resolver.get().is_some()
    }
}

/// The keyless SEC EDGAR company-facts adapter. Mirrors the gated adapters' shape
/// (`http` + `base_url` + `progress`), minus the API key.
pub struct SecEdgarSource {
    http: reqwest::blocking::Client,
    base_url: String,
    /// The `www.sec.gov` host serving `company_tickers.json` — a distinct base from
    /// the `data.sec.gov` API host, with its own test seam.
    tickers_base_url: String,
    progress: Arc<RunContext>,
}

impl SecEdgarSource {
    /// Build the adapter. The User-Agent SEC asks for is set on the client once.
    pub fn new() -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(SEC_USER_AGENT)
            .build()
            .context("building the SEC EDGAR HTTP client")?;
        Ok(Self {
            http,
            base_url: SEC_DATA_BASE.to_string(),
            tickers_base_url: SEC_TICKERS_BASE.to_string(),
            progress: RunContext::noop(),
        })
    }

    /// Point the adapter at a mock base URL for the offline round-trip test. Trailing
    /// slash trimmed so the joined path's leading slash doesn't double up. Points both
    /// hosts at the mock, since a test exercises one endpoint at a time (crate-visible
    /// so the portfolio job's slot-ordering test can drive the real fetch path).
    #[cfg(test)]
    pub(crate) fn with_base_url(mut self, base_url: &str) -> Self {
        let base = base_url.trim_end_matches('/').to_string();
        self.tickers_base_url = base.clone();
        self.base_url = base;
        self
    }

    /// Fetch the raw `company_tickers.json` body (the caller parses and caches it —
    /// [`load_cik_resolver`]). A transport error or non-2xx returns `Err`; resolution
    /// then falls back fail-soft.
    pub fn fetch_company_tickers(&self) -> Result<String> {
        if self.progress.is_cancelled() {
            anyhow::bail!("SEC ticker-map fetch skipped (run cancelled)");
        }
        let url = format!("{}{SEC_TICKERS_PATH}", self.tickers_base_url);
        self.progress
            .request_started("SEC", "company-tickers", "all", "SEC ticker→CIK map");
        let result = (|| -> Result<String> {
            let (status, body) =
                crate::http_retry::send_with_retry("SEC", || self.http.get(&url))?;
            if !(200..300).contains(&status) {
                anyhow::bail!("SEC returned {status} for company_tickers.json");
            }
            Ok(body)
        })();
        match &result {
            Ok(_) => self.progress.request_finished(
                "SEC",
                "company-tickers",
                "all",
                "SEC ticker→CIK map",
                "ok",
                None,
            ),
            Err(e) => self.progress.request_finished(
                "SEC",
                "company-tickers",
                "all",
                "SEC ticker→CIK map",
                "failed",
                Some(e.to_string()),
            ),
        }
        result
    }

    /// Attach a live run context so each fetch streams a tracker row.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Fetch the company-facts JSON for a CIK and shape it into [`CompanyFacts`]. A
    /// transport error or non-2xx returns `Err`; the caller (the dossier) treats that
    /// fail-soft, since SEC supplements FMP rather than gating the run.
    pub fn fetch_company_facts(&self, cik10: &str) -> Result<CompanyFacts> {
        // Cancel checkpoint before the request: a cancel already requested skips the
        // network (no request, so no tracker row) and surfaces as an error the job's
        // cancel path classifies as a user stop.
        if self.progress.is_cancelled() {
            anyhow::bail!("SEC fetch skipped (run cancelled)");
        }
        let path = company_facts_path(cik10);
        let url = format!("{}{path}", self.base_url);
        self.progress
            .request_started("SEC", "company-facts", cik10, "SEC company facts");
        let result = (|| -> Result<CompanyFacts> {
            let (status, body) =
                crate::http_retry::send_with_retry("SEC", || self.http.get(&url))?;
            if !(200..300).contains(&status) {
                anyhow::bail!("SEC EDGAR returned {status}");
            }
            let value: Value = serde_json::from_str(&body).context("parsing SEC company facts")?;
            Ok(facts_from_value(&value))
        })();
        match &result {
            Ok(_) => self.progress.request_finished(
                "SEC",
                "company-facts",
                cik10,
                "SEC company facts",
                "ok",
                None,
            ),
            Err(e) => self.progress.request_finished(
                "SEC",
                "company-facts",
                cik10,
                "SEC company facts",
                "failed",
                Some(e.to_string()),
            ),
        }
        result
    }

    /// A filer's recent filings from the submissions index, newest first — the quick
    /// check's per-stock EDGAR filing sweep. `Err` on transport / non-2xx / parse
    /// failure so the caller types the filing family `unknown` rather than reading a
    /// failed sweep as no-new-filings.
    pub fn fetch_recent_filings(&self, cik10: &str) -> Result<Vec<RecentFiling>> {
        if self.progress.is_cancelled() {
            anyhow::bail!("SEC submissions fetch skipped (run cancelled)");
        }
        let url = format!("{}{}", self.base_url, submissions_path(cik10));
        self.progress
            .request_started("SEC", "submissions", cik10, "SEC recent filings");
        let result = (|| -> Result<Vec<RecentFiling>> {
            let (status, body) =
                crate::http_retry::send_with_retry("SEC", || self.http.get(&url))?;
            if !(200..300).contains(&status) {
                anyhow::bail!("SEC submissions returned {status}");
            }
            let value: Value =
                serde_json::from_str(&body).context("parsing SEC submissions")?;
            recent_filings_from_value(&value)
        })();
        match &result {
            Ok(_) => self.progress.request_finished(
                "SEC",
                "submissions",
                cik10,
                "SEC recent filings",
                "ok",
                None,
            ),
            Err(e) => self.progress.request_finished(
                "SEC",
                "submissions",
                cik10,
                "SEC recent filings",
                "failed",
                Some(e.to_string()),
            ),
        }
        result
    }
}

/// One recent filing from the submissions index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentFiling {
    /// The form type as EDGAR reports it (`10-Q`, `8-K`, `10-K/A`, `4`, …).
    pub form: String,
    /// The filing date, ISO.
    pub filing_date: String,
    /// The filer-declared item codes for an 8-K (`["4.01", "9.01"]` — the
    /// submissions feed's `items` column, split on commas), empty on other forms.
    /// Filer-declared structured metadata, mandatory in the 8-K submission types,
    /// so an 8-K row filed after 2004 reliably carries its items — what lets the
    /// hard-forensic producer classify an Item 4.01 / 4.02 filing without
    /// fetching the document (`docs/data-sources.md §SEC EDGAR`).
    /// `None` = the column was absent or this row's entry unreadable — the row is
    /// **unclassifiable**, which the forensic sweep must surface as `unknown`,
    /// never fold into a clean result (the fabricated-clear failure mode);
    /// `Some(vec![])` = the column served an honestly empty entry (a non-8-K).
    pub items: Option<Vec<String>>,
    /// The filing's accession number — the event record's source lineage.
    pub accession: String,
}

/// Shape the submissions body's `filings.recent` parallel arrays (`form[i]` ↔
/// `filingDate[i]`) into rows, newest first as EDGAR serves them. A body without the `filings.recent`
/// arrays is malformed or drifted (the submissions schema always carries them,
/// empty arrays included, for every filer) — `Err`, never an empty success, so
/// the sweep types the filing family `unknown` instead of reading
/// "no new filings" off a body it couldn't interpret. The form + date legs are
/// **strict**: unpaired arrays, a non-string leg, or an undatable
/// `filingDate` all `Err` rather than dropping the row — a silently dropped
/// 8-K would fold into a clean forensic sweep, and a garbage date compares
/// lexically against the classifier's lookback bound (the fabricated-clear
/// rule, `crate::portfolio::ForensicFilingState`). The `accessionNumber`
/// column stays lenient per row (absent → empty lineage); the `items` column
/// is honest per row — an absent column or an unreadable entry yields `None`
/// (unclassifiable), never an empty list.
fn recent_filings_from_value(value: &Value) -> Result<Vec<RecentFiling>> {
    let recent = &value["filings"]["recent"];
    let (Some(forms), Some(dates)) = (recent["form"].as_array(), recent["filingDate"].as_array())
    else {
        anyhow::bail!(
            "SEC submissions body lacked the filings.recent arrays — malformed or drifted response"
        );
    };
    if forms.len() != dates.len() {
        anyhow::bail!(
            "SEC submissions form/filingDate arrays are unpaired ({} vs {}) — malformed or \
             drifted response",
            forms.len(),
            dates.len()
        );
    }
    let items = recent["items"].as_array();
    let accessions = recent["accessionNumber"].as_array();
    forms
        .iter()
        .zip(dates.iter())
        .enumerate()
        .map(|(i, (form, date))| {
            let form = form
                .as_str()
                .with_context(|| format!("SEC submissions row {i}: non-string form leg"))?;
            let filing_date = date
                .as_str()
                .with_context(|| format!("SEC submissions row {i}: non-string filingDate leg"))?;
            // Store the CANONICAL fixed-width render, never the source text:
            // chrono accepts unpadded fields, and a datable-but-noncanonical
            // "2026-9-30" sorts lexically after "2026-10-01" — exactly the
            // comparison the forensic lookback bound makes (the same hazard
            // `fmp::canonical_date` guards; Codex 2026-08-20 round 3).
            let filing_date = chrono::NaiveDate::parse_from_str(filing_date, "%Y-%m-%d")
                .with_context(|| {
                    format!("SEC submissions row {i}: undatable filingDate {filing_date:?}")
                })?
                .format("%Y-%m-%d")
                .to_string();
            Ok(RecentFiling {
                form: form.to_string(),
                filing_date,
                items: items
                    .and_then(|a| a.get(i))
                    .and_then(Value::as_str)
                    .map(|s| {
                        s.split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect()
                    }),
                accession: accessions
                    .and_then(|a| a.get(i))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

// ---- Hard-forensic filing kinds (the item-classified producer) -----------------

/// The typed hard-forensic event kinds — the shared producer contract
/// (`docs/trade-opportunities-workflow.md §Step 5c`; Portfolio's engine and
/// continuity seams consume the same records).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ForensicEventKind {
    /// An Item 4.02 non-reliance 8-K.
    Restatement,
    /// An Item 4.01 auditor-change 8-K.
    AuditorChange,
    /// The research-fed kind — no structured enumeration exists, so it enters
    /// only as a validated `forensic_event` research claim (the 6d channel,
    /// merged by `pipeline::merge_research_forensic_event`). This filings
    /// classifier never emits it.
    Fraud,
}

impl ForensicEventKind {
    pub fn label(self) -> &'static str {
        match self {
            ForensicEventKind::Restatement => "restatement (Item 4.02 non-reliance)",
            ForensicEventKind::AuditorChange => "auditor change (Item 4.01)",
            ForensicEventKind::Fraud => "fraud (research-fed)",
        }
    }
}

/// One typed hard-forensic event — `{ event kind, issuer, event / filing date,
/// source lineage, confidence }`, the producer contract's record. The filing
/// kinds are engine-detected and model-free; a bare model assertion is never one
/// of these.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ForensicEvent {
    pub kind: ForensicEventKind,
    /// The issuer, as the consuming job identifies it (the holding's symbol).
    pub issuer: String,
    /// The filing date, ISO — the event date the hard rule's lookback reads.
    pub filing_date: String,
    /// Source lineage — the form type plus accession number of the classifying
    /// filing.
    pub source: String,
    /// How the event was established. Filing kinds are filer-declared item
    /// codes, structural rather than judged.
    pub confidence: String,
}

/// Classify the hard-forensic **filing kinds** from an already-fetched
/// submissions sweep: an 8-K (or 8-K/A) whose filer-declared items carry `4.02`
/// (non-reliance restatement) or `4.01` (auditor change), filed on or after
/// `since` (ISO date, inclusive — the consumer's lookback bound). Model-free and
/// pure. `Err` when any in-lookback 8-K row is **unclassifiable** (its `items`
/// read `None` — the column absent or the entry unreadable): the caller must
/// type the sweep `unknown`, because "couldn't read the items" folded into a
/// clean result is exactly the fabricated clear the contract forbids. The fraud
/// kind never comes from here (research-fed only).
pub fn forensic_events_from_filings(
    issuer: &str,
    filings: &[RecentFiling],
    since: &str,
) -> std::result::Result<Vec<ForensicEvent>, String> {
    let in_scope = filings.iter().filter(|f| {
        (f.form == "8-K" || f.form == "8-K/A") && f.filing_date.as_str() >= since
    });
    let mut events = Vec::new();
    for f in in_scope {
        let Some(items) = &f.items else {
            return Err(format!(
                "8-K filed {} carries no readable items column — cannot classify",
                f.filing_date
            ));
        };
        for item in items {
            let kind = match item.as_str() {
                "4.02" => ForensicEventKind::Restatement,
                "4.01" => ForensicEventKind::AuditorChange,
                _ => continue,
            };
            events.push(ForensicEvent {
                kind,
                issuer: issuer.to_string(),
                filing_date: f.filing_date.clone(),
                source: if f.accession.is_empty() {
                    format!("{} filing", f.form)
                } else {
                    format!("{} accession {}", f.form, f.accession)
                },
                confidence: "filing-declared item code".to_string(),
            });
        }
    }
    Ok(events)
}

/// Candidate GAAP concept names for revenue — the tag changed across taxonomy
/// versions, so try the newer name first and fall back.
const REVENUE_CONCEPTS: &[&str] = &[
    "RevenueFromContractWithCustomerExcludingAssessedTax",
    "Revenues",
    "SalesRevenueNet",
];

/// Shape an `/api/xbrl/companyfacts` body into [`CompanyFacts`]. Pure, so the
/// envelope contract is unit-testable without a live call. The prior-year revenue
/// deliberately comes from the **same concept ladder rung** that supplied the latest
/// print — a growth read across two different revenue tags would compare different
/// economics.
fn facts_from_value(value: &Value) -> CompanyFacts {
    let (revenue, revenue_prior) = REVENUE_CONCEPTS
        .iter()
        .find_map(|c| {
            let (latest, prior) = latest_two_annual_usd(value, c);
            latest.map(|l| (Some(l), prior))
        })
        .unwrap_or((None, None));
    CompanyFacts {
        revenue,
        revenue_prior,
        gross_profit: latest_annual_usd(value, "GrossProfit"),
        net_income: latest_annual_usd(value, "NetIncomeLoss"),
        total_assets: latest_annual_usd(value, "Assets"),
        stockholders_equity: latest_annual_usd(value, "StockholdersEquity"),
    }
}

/// The latest annual (form `10-K`, full-year) USD value for one GAAP concept, picked
/// by the most recent `end` date. `None` when the concept is absent or has no annual
/// USD datapoint. Reading only the 10-K full-year rows avoids mixing a quarterly
/// figure into an annual metric.
fn latest_annual_usd(value: &Value, concept: &str) -> Option<i64> {
    let (latest, _) = latest_two_annual_usd(value, concept);
    latest
}

/// Instant (balance) concepts — a point-in-time fact carries no `start`, so the
/// annual-duration span check does not apply. Everything else is treated as a
/// duration (flow) concept and FAILS CLOSED without a parseable ~annual span:
/// the safe default for any concept added later, since a mistakenly-instant
/// read only skips a check while a mistakenly-duration read fabricates an
/// annual value from a stub period.
const INSTANT_CONCEPTS: &[&str] = &["Assets", "StockholdersEquity"];

/// The latest **two** annual full-year USD values for one GAAP concept, by distinct
/// `end` date descending — the second is the prior fiscal year's print (a 10-K
/// carries its comparative-year rows under the same concept, which is what makes the
/// prior read possible without a second request). Duplicate rows for one period end
/// (an original filing plus a later 10-K's comparative) collapse to the
/// **latest-filed** row — a later filing that restates the period (recast for a
/// disposal, an accounting change) supersedes the original print; rows without a
/// `filed` date fall back to array order, where EDGAR lists later filings later.
fn latest_two_annual_usd(value: &Value, concept: &str) -> (Option<i64>, Option<i64>) {
    let Some(units) = value
        .pointer(&format!("/facts/us-gaap/{concept}/units/USD"))
        .and_then(Value::as_array)
    else {
        return (None, None);
    };
    // (end, filed, array index, val) — sorted so the row to keep per period end
    // comes first: end desc, then filed desc (absent filed sorts behind any
    // present one), then array position desc as the filing-order proxy.
    let mut dated: Vec<(String, Option<String>, usize, i64)> = units
        .iter()
        .enumerate()
        // Prefix match: a 10-K/A restating the year is the most direct
        // supersession vehicle — an exact "10-K" test would keep serving the
        // withdrawn original print until the next annual report's comparative.
        .filter(|(_, row)| {
            row.get("form")
                .and_then(Value::as_str)
                .is_some_and(|f| f.starts_with("10-K"))
        })
        // Prefer full-year datapoints; many 10-K rows carry `"fp":"FY"`.
        .filter(|(_, row)| {
            row.get("fp")
                .and_then(Value::as_str)
                .map(|fp| fp == "FY")
                .unwrap_or(true)
        })
        // Duration facts must span roughly a year: company-facts arrays mix
        // sub-annual durations under the same concept and form, and a Q4/stub
        // row sharing the FY `end` date would otherwise win on a tie-break and
        // masquerade as the annual value. CONCEPT-AWARE and fail-closed: a
        // duration concept's row with an absent or unparseable `start` is
        // excluded (a pass-through would readmit exactly the stub rows the
        // filter exists to stop), while instant concepts skip the check —
        // point-in-time facts legitimately carry no `start`.
        .filter(|(_, row)| {
            if INSTANT_CONCEPTS.contains(&concept) {
                return true;
            }
            row.get("start")
                .and_then(Value::as_str)
                .zip(row.get("end").and_then(Value::as_str))
                .and_then(|(s, e)| {
                    let s = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
                    let e = chrono::NaiveDate::parse_from_str(e, "%Y-%m-%d").ok()?;
                    Some((e - s).num_days())
                })
                .is_some_and(|d| (350..=380).contains(&d))
        })
        .filter_map(|(idx, row)| {
            let end = row.get("end").and_then(Value::as_str)?;
            let filed = row
                .get("filed")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
            let val = row.get("val").and_then(Value::as_i64)?;
            Some((end.to_string(), filed, idx, val))
        })
        .collect();
    dated.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| b.2.cmp(&a.2))
    });
    dated.dedup_by(|a, b| a.0 == b.0);
    let mut iter = dated.into_iter();
    (
        iter.next().map(|(_, _, _, v)| v),
        iter.next().map(|(_, _, _, v)| v),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    fn facts_body() -> &'static str {
        // Two revenue datapoints (older + newer 10-K, the older year appearing twice:
        // the original print and a later-filed 10-K comparative that RESTATES it) and
        // one each for the rest. The parser must pick the latest annual by `end`
        // date, the prior year from the same concept's second-distinct end, and the
        // latest-FILED row where one period end appears twice.
        r#"{
          "facts": {
            "us-gaap": {
              "RevenueFromContractWithCustomerExcludingAssessedTax": {
                "units": { "USD": [
                  {"start":"2023-10-01","end":"2024-09-28","val":391035000000,"form":"10-K","fp":"FY","filed":"2024-11-01"},
                  {"start":"2022-10-02","end":"2023-09-30","val":383285000000,"form":"10-K","fp":"FY","filed":"2023-11-03"},
                  {"start":"2022-10-02","end":"2023-09-30","val":380000000000,"form":"10-K","fp":"FY","filed":"2024-11-01"}
                ]}
              },
              "Revenues": {
                "units": { "USD": [
                  {"start":"2021-09-26","end":"2022-09-24","val":394328000000,"form":"10-K","fp":"FY"}
                ]}
              },
              "NetIncomeLoss": {
                "units": { "USD": [
                  {"start":"2023-10-01","end":"2024-09-28","val":93736000000,"form":"10-K","fp":"FY"},
                  {"start":"2024-03-31","end":"2024-06-29","val":21448000000,"form":"10-Q","fp":"Q3"}
                ]}
              },
              "StockholdersEquity": {
                "units": { "USD": [ {"end":"2024-09-28","val":56950000000,"form":"10-K","fp":"FY"} ] }
              }
            }
          }
        }"#
    }

    #[test]
    fn parses_latest_annual_facts_and_ignores_quarterly_rows() {
        let value: Value = serde_json::from_str(facts_body()).unwrap();
        let facts = facts_from_value(&value);
        // Latest annual revenue (the 2024 10-K), not the prior year.
        assert_eq!(facts.revenue, Some(391_035_000_000));
        // The prior year comes from the SAME concept's second-distinct end; the
        // duplicated period end collapses to the latest-FILED row (the later 10-K's
        // restated comparative supersedes the original print), and the older
        // `Revenues` concept (a different tag, different economics) never mixes in.
        assert_eq!(facts.revenue_prior, Some(380_000_000_000));
        // The 10-Q net-income row is filtered out; the 10-K stands.
        assert_eq!(facts.net_income, Some(93_736_000_000));
        assert_eq!(facts.stockholders_equity, Some(56_950_000_000));
        // A concept that wasn't reported stays absent rather than fabricated.
        assert_eq!(facts.total_assets, None);
        assert!(!facts.is_empty());
    }

    #[test]
    fn amendments_supersede_and_sub_annual_durations_are_excluded() {
        // A 10-K/A restating the year is the most direct supersession vehicle —
        // the prefix match must let its later-filed row win the period-end
        // dedup — and a Q4-duration fact sharing the FY `end` date on a 10-K
        // row must not masquerade as the annual value (the tie-break would
        // otherwise fall to serialization order).
        let body = r#"{
          "facts": {
            "us-gaap": {
              "RevenueFromContractWithCustomerExcludingAssessedTax": {
                "units": { "USD": [
                  {"start":"2023-10-01","end":"2024-09-28","val":391035000000,"form":"10-K","fp":"FY","filed":"2024-11-01"},
                  {"start":"2023-10-01","end":"2024-09-28","val":359752200000,"form":"10-K/A","fp":"FY","filed":"2025-03-15"},
                  {"start":"2024-06-30","end":"2024-09-28","val":94930000000,"form":"10-K","fp":"FY","filed":"2024-11-01"},
                  {"end":"2024-09-28","val":1,"form":"10-K","fp":"FY","filed":"2025-06-01"},
                  {"start":"2022-10-02","end":"2023-09-30","val":383285000000,"form":"10-K","fp":"FY","filed":"2023-11-03"}
                ]}
              }
            }
          }
        }"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let facts = facts_from_value(&value);
        // The amendment's restated print wins (later filed, same period end);
        // the Q4-duration row (90 days) is excluded outright, and so is the
        // start-less duration row — filed latest, it would WIN the dedup under
        // a fail-open span check (duration concepts fail closed; only instant
        // concepts legitimately omit `start`).
        assert_eq!(facts.revenue, Some(359_752_200_000));
        assert_eq!(facts.revenue_prior, Some(383_285_000_000));
    }

    #[test]
    fn fetch_round_trips_a_200_into_company_facts() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: facts_body(),
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let facts = sec.fetch_company_facts("0000320193").unwrap();
        assert_eq!(facts.revenue, Some(391_035_000_000));
        assert_eq!(
            server.request_paths(),
            vec!["/api/xbrl/companyfacts/CIK0000320193.json".to_string()]
        );
    }

    #[test]
    fn fetch_surfaces_a_non_2xx_as_an_error() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 404,
            headers: vec![],
            body: "not found",
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let err = sec.fetch_company_facts("0000000000").unwrap_err();
        assert!(err.to_string().contains("404"), "{err}");
    }

    #[test]
    fn recent_filings_round_trip_the_submissions_parallel_arrays() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"cik":"320193","filings":{"recent":{
                "form":["4","10-Q","8-K"],
                "filingDate":["2026-08-01","2026-07-31","2026-07-30"],
                "accessionNumber":["a","b","c"]
            }}}"#,
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let filings = sec.fetch_recent_filings("0000320193").unwrap();
        assert_eq!(filings.len(), 3);
        assert_eq!(filings[0].form, "4");
        assert_eq!(filings[1].form, "10-Q");
        assert_eq!(filings[1].filing_date, "2026-07-31");
        assert_eq!(
            server.request_paths(),
            vec!["/submissions/CIK0000320193.json".to_string()]
        );
    }

    #[test]
    fn recent_filings_parse_items_honestly_per_row() {
        // The items column is the 8-K classification surface (comma-separated,
        // filer-declared): a served entry parses (an empty entry is an honest
        // `Some(vec![])`), while an absent column reads `None` — unclassifiable,
        // for the forensic sweep to surface as unknown, never fold into clean.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"cik":"320193","filings":{"recent":{
                "form":["8-K","10-Q"],
                "filingDate":["2026-08-01","2026-07-31"],
                "items":["4.02,9.01",""],
                "accessionNumber":["0000320193-26-000042","0000320193-26-000041"]
            }}}"#,
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let filings = sec.fetch_recent_filings("0000320193").unwrap();
        assert_eq!(
            filings[0].items,
            Some(vec!["4.02".to_string(), "9.01".to_string()])
        );
        assert_eq!(filings[0].accession, "0000320193-26-000042");
        assert_eq!(filings[1].items, Some(vec![]));

        // No items / accession arrays at all: rows still parse (the form + date
        // hard floor holds), but every row's items read `None`.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"cik":"320193","filings":{"recent":{
                "form":["8-K"],
                "filingDate":["2026-08-01"]
            }}}"#,
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let filings = sec.fetch_recent_filings("0000320193").unwrap();
        assert_eq!(filings.len(), 1);
        assert_eq!(filings[0].items, None);
        assert!(filings[0].accession.is_empty());
    }

    #[test]
    fn malformed_form_or_date_legs_error_never_drop_the_row() {
        // A null date leg on an in-scope 8-K must not vanish into a clean
        // sweep, and a garbage date must not survive to compare lexically
        // against the classifier's lookback bound — the whole fetch errors
        // onto the callers' unknown postures (Codex 2026-08-20 round 2,
        // finding 1).
        let bodies = [
            // Null date leg.
            r#"{"filings":{"recent":{"form":["8-K"],"filingDate":[null]}}}"#,
            // Non-date date leg (lexically above any ISO lookback bound).
            r#"{"filings":{"recent":{"form":["8-K"],"filingDate":["not-a-date"]}}}"#,
            // Null form leg.
            r#"{"filings":{"recent":{"form":[null],"filingDate":["2026-08-01"]}}}"#,
            // Unpaired arrays.
            r#"{"filings":{"recent":{"form":["8-K","10-Q"],"filingDate":["2026-08-01"]}}}"#,
        ];
        for body in bodies {
            let server = MockHttp::serve(vec![Canned::Reply {
                status: 200,
                headers: vec![],
                body: Box::leak(body.to_string().into_boxed_str()),
            }]);
            let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
            assert!(sec.fetch_recent_filings("0000320193").is_err(), "{body}");
        }

        // A datable-but-noncanonical date is stored in its canonical render —
        // "2026-9-30" would otherwise sort lexically after "2026-10-01",
        // exactly the classifier's lookback comparison.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"filings":{"recent":{"form":["8-K"],"filingDate":["2026-9-30"]}}}"#,
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let filings = sec.fetch_recent_filings("0000320193").unwrap();
        assert_eq!(filings[0].filing_date, "2026-09-30");
    }

    #[test]
    fn forensic_classifier_types_401_402_inside_the_lookback_only() {
        let filing = |form: &str, date: &str, items: &[&str]| RecentFiling {
            form: form.into(),
            filing_date: date.into(),
            items: Some(items.iter().map(|s| s.to_string()).collect()),
            accession: "acc-1".into(),
        };
        let filings = vec![
            // Item 4.02 → restatement; the companion 9.01 produces nothing.
            filing("8-K", "2026-08-01", &["4.02", "9.01"]),
            // An amended 8-K classifies too.
            filing("8-K/A", "2026-07-15", &["4.01"]),
            // A non-forensic 8-K item → no event.
            filing("8-K", "2026-07-10", &["2.02"]),
            // Outside the lookback → filtered.
            filing("8-K", "2024-01-05", &["4.02"]),
            // A 10-Q never classifies, whatever its items column carries.
            filing("10-Q", "2026-07-31", &["4.02"]),
        ];
        let events = forensic_events_from_filings("ACME", &filings, "2025-08-20").unwrap();
        assert_eq!(events.len(), 2, "{events:?}");
        assert_eq!(events[0].kind, ForensicEventKind::Restatement);
        assert_eq!(events[0].issuer, "ACME");
        assert_eq!(events[0].filing_date, "2026-08-01");
        assert!(events[0].source.contains("acc-1"), "{}", events[0].source);
        assert_eq!(events[1].kind, ForensicEventKind::AuditorChange);
        // Tightening the lookback drops the older auditor change, keeping only
        // the newer restatement — the bound is inclusive on `since`.
        assert_eq!(
            forensic_events_from_filings("ACME", &filings, "2026-07-20")
                .unwrap()
                .len(),
            1
        );

        // An in-lookback 8-K whose items column is unreadable makes the whole
        // sweep unclassifiable — `Err`, never a clean or partial result (the
        // fabricated-clear rule). Outside the lookback it is ignorable.
        let mut with_unreadable = filings.clone();
        with_unreadable.push(RecentFiling {
            form: "8-K".into(),
            filing_date: "2026-08-10".into(),
            items: None,
            accession: String::new(),
        });
        assert!(forensic_events_from_filings("ACME", &with_unreadable, "2025-08-20").is_err());
        assert!(forensic_events_from_filings("ACME", &with_unreadable, "2026-08-11").is_ok());
    }

    #[test]
    fn recent_filings_error_on_non_2xx_never_reading_as_no_new_filings() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 404,
            headers: vec![],
            body: "not found",
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        assert!(sec.fetch_recent_filings("0000000000").is_err());
    }

    #[test]
    fn recent_filings_error_on_a_200_missing_the_recent_arrays() {
        // Valid JSON without `filings.recent` is schema drift or a malformed
        // response — `Err`, never an empty "no new filings" success.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"cik":"320193","name":"Apple Inc."}"#,
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let err = sec.fetch_recent_filings("0000320193").unwrap_err();
        assert!(err.to_string().contains("filings.recent"), "{err}");
    }

    fn tickers_body() -> &'static str {
        r#"{
          "0": {"cik_str": 320193, "ticker": "AAPL", "title": "Apple Inc."},
          "1": {"cik_str": 789019, "ticker": "MSFT", "title": "MICROSOFT CORP"},
          "2": {"cik_str": 34088, "ticker": "XOM", "title": "EXXON MOBIL CORP"},
          "3": {"ticker": "BROKEN"}
        }"#
    }

    #[test]
    fn resolver_parses_the_full_map_and_zero_pads_ciks() {
        let resolver = CikResolver::from_json(tickers_body()).unwrap();
        assert_eq!(resolver.len(), 3, "the malformed row is skipped, not fabricated");
        assert_eq!(resolver.resolve("aapl"), Some("0000320193"));
        assert_eq!(resolver.resolve("XOM"), Some("0000034088"), "short CIKs zero-pad to 10");
        assert_eq!(resolver.resolve("ZZZZ"), None);
    }

    #[test]
    fn ticker_map_fetch_round_trips_and_hits_the_files_path() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: tickers_body(),
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let body = sec.fetch_company_tickers().unwrap();
        assert!(CikResolver::from_json(&body).unwrap().resolve("MSFT").is_some());
        assert_eq!(
            server.request_paths(),
            vec!["/files/company_tickers.json".to_string()]
        );
    }

    #[test]
    fn load_cik_resolver_fetches_then_reuses_the_fresh_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("sec_company_tickers.json");
        // First load: no cache → fetch → cache written.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: tickers_body(),
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let resolver = load_cik_resolver(&cache, &sec);
        assert_eq!(resolver.resolve("AAPL"), Some("0000320193"));
        assert!(cache.exists(), "the fetched map is cached beside the db");
        // Second load: the fresh cache serves without any request — the mock has no
        // second canned reply, so a fetch attempt would fail and fall to empty.
        let sec_offline = SecEdgarSource::new().unwrap().with_base_url("http://127.0.0.1:1");
        let resolver = load_cik_resolver(&cache, &sec_offline);
        assert_eq!(resolver.resolve("MSFT"), Some("0000789019"));
    }

    #[test]
    fn load_cik_resolver_falls_back_to_a_stale_cache_then_empty() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("sec_company_tickers.json");
        std::fs::write(&cache, tickers_body()).unwrap();
        // Age the cache past the freshness window so a refresh is attempted; the
        // unreachable fetch then falls back to the stale cache rather than empty.
        age_past_freshness(&cache);
        let sec_offline = SecEdgarSource::new().unwrap().with_base_url("http://127.0.0.1:1");
        let resolver = load_cik_resolver(&cache, &sec_offline);
        assert_eq!(resolver.resolve("AAPL"), Some("0000320193"), "stale beats empty");
        // No cache at all → the empty fail-soft floor.
        let resolver = load_cik_resolver(&dir.path().join("missing.json"), &sec_offline);
        assert!(resolver.is_empty());
    }

    /// Age a cache file past the freshness window so a refresh is attempted.
    fn age_past_freshness(cache: &std::path::Path) {
        let stale = std::time::SystemTime::now() - (CIK_CACHE_MAX_AGE + Duration::from_secs(60));
        let file = std::fs::File::options().append(true).open(cache).unwrap();
        file.set_modified(stale).unwrap();
    }

    /// Documents the bail: the ticker-map refresh honors the shared cancel flag
    /// like every SEC request, so under a set flag it makes **no request** and
    /// falls to the stale map. This is exactly why the live jobs must not load
    /// the resolver before the slot clears the flag (`reset_cancel`) — an eager
    /// load after a cancelled run would ship this stale (or empty) map into the
    /// whole run without a single request row.
    #[test]
    fn load_cik_resolver_under_a_set_cancel_flag_and_stale_cache_returns_the_stale_map() {
        use std::sync::atomic::AtomicBool;
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("sec_company_tickers.json");
        std::fs::write(&cache, tickers_body()).unwrap();
        age_past_freshness(&cache);
        // A mock that WOULD serve a fresh map — it must see no connection.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: tickers_body(),
        }]);
        let cancelled = RunContext::new(
            "cancelled-earlier",
            Arc::new(crate::progress::NoopReporter),
            Arc::new(AtomicBool::new(true)),
        );
        let sec = SecEdgarSource::new()
            .unwrap()
            .with_base_url(&server.base_url)
            .with_context(cancelled);
        let resolver = load_cik_resolver(&cache, &sec);
        assert_eq!(
            resolver.resolve("AAPL"),
            Some("0000320193"),
            "the stale cache is served, not empty"
        );
        assert_eq!(server.attempts(), 0, "a set cancel flag skips the refresh fetch");
        // With no cache the same bail lands on the empty floor.
        let resolver = load_cik_resolver(&dir.path().join("missing.json"), &sec);
        assert!(resolver.is_empty());
        assert_eq!(server.attempts(), 0);
    }

    /// The lazy carrier: constructing it performs no I/O; the first `get` loads
    /// (one request), the second serves the memoized map (no request).
    #[test]
    fn lazy_cik_resolver_fetches_on_first_use_only() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("sec_company_tickers.json");
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: tickers_body(),
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let lazy = LazyCikResolver::new(&cache);
        assert!(!lazy.is_loaded());
        assert_eq!(server.attempts(), 0, "construction fetches nothing");
        assert_eq!(lazy.resolve(&sec, "MSFT"), Some("0000789019"));
        assert!(lazy.is_loaded());
        assert_eq!(server.attempts(), 1);
        // Memoized: no second connection (the mock has no second reply anyway).
        assert_eq!(lazy.resolve(&sec, "AAPL"), Some("0000320193"));
        assert_eq!(server.attempts(), 1);
        // A preloaded resolver never touches the source.
        let pre = LazyCikResolver::preloaded(CikResolver::from_json(tickers_body()).unwrap());
        assert!(pre.is_loaded());
        assert_eq!(pre.resolve(&sec, "XOM"), Some("0000034088"));
        assert_eq!(server.attempts(), 1);
    }

    #[test]
    fn cik_cache_lives_beside_the_database() {
        assert_eq!(
            cik_cache_path_beside(std::path::Path::new("/data/app/market_signal.db")),
            std::path::PathBuf::from("/data/app/sec_company_tickers.json")
        );
        assert_eq!(
            cik_cache_path_beside(std::path::Path::new("market_signal.db")),
            std::path::PathBuf::from("sec_company_tickers.json")
        );
    }
}
