//! Source registry and evidence tiers (`docs/data-sources.md §Source registry
//! and evidence tiers`; the informs-never-gates rule in `docs/web-research.md
//! §Source quality and evidence weighting`).
//!
//! The web loop reaches an unbounded set of domains, so the suite weighs
//! *where* evidence comes from, not only what it says. The registry is a
//! **thin override over heuristic defaults** — the long tail of the web is
//! never registered and is scored by default rules; the registry pins the
//! handful of domains whose treatment must be deliberate (primary sources,
//! known specialists, denied junk).
//!
//! The load-bearing rule: **tiers grade, `deny` excludes, nothing between is
//! gated.** A low tier weights conviction down; only the explicit deny list
//! drops a hit, and denying junk that isn't evidence at all (SEO stock mills,
//! AI-generated quote pages) is keeping spam out of the evidence base, not
//! gating on quality. The seed membership below is drafted and accrues from
//! live-run evidence; a user-facing override surface is deferred
//! (`docs/configuration.md §Web Research`).

use serde::{Deserialize, Serialize};

/// How the fetch layer should treat a domain (`docs/data-sources.md` — the
/// `extractionProfile` field). Seeded here; the learned per-domain state the
/// telemetry accumulates lives in the web-research source-state store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionProfile {
    ApiOrHtml,
    Html,
    JsRequired,
}

/// Whether full text depends on a connected subscription. Connected Sources is
/// deferred by ruling (2026-08-23), so `Connected` has no consumer this slice —
/// the field keeps the documented registry shape so the deferral costs no
/// schema change later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialMode {
    None,
    Connected,
}

/// Per-lane treatment (`docs/web-research.md §Source quality and evidence
/// weighting` — Lane policy). Portfolio runs the per-holding *validation*
/// lane only; the discovery lane's soft preference joins with Trade
/// Opportunities. `SentimentOnly` marks the Tier-5 read: sentiment signal,
/// never fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanePolicy {
    Evidence,
    SentimentOnly,
}

/// One registered domain's metadata (`docs/data-sources.md §Source registry
/// and evidence tiers` — Per-domain metadata).
#[derive(Debug, Clone, PartialEq)]
pub struct RegistryEntry {
    /// Evidence tier 0–5 (0 = primary/deterministic … 5 = sentiment/noise).
    pub tier: u8,
    /// What the source is good for — per-kind, never one reputation number
    /// (SEC is authoritative for filings, useless for market narrative).
    pub evidence_kinds: &'static [&'static str],
    pub lane_policy: LanePolicy,
    pub credential_mode: CredentialMode,
    /// How stale a hit may be before it is down-weighted for this kind —
    /// long for a filing, short for a price/ASP read.
    pub freshness_sla_days: u32,
    pub extraction_profile: ExtractionProfile,
    /// Full text depends on a subscription (down-weights the *expectation* of
    /// extraction yield; never a gate).
    pub paywall: bool,
}

/// The registry's answer for a host: graded metadata, or the one categorical
/// exclusion.
#[derive(Debug, Clone, PartialEq)]
pub enum SourcePolicy {
    Graded(RegistryEntry),
    /// Dropped at search-filter and fetch-gate with the recorded reason.
    Deny(&'static str),
}

/// The app-computed half of the per-document evidence annotation
/// (`docs/web-research.md §Source quality and evidence weighting` — "the
/// evidence annotation, split by who can know it"). Model-derived judgment
/// (claim specificity, contradiction flags) stays model-side and is never
/// dressed up as app-computed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceAnnotation {
    /// Evidence tier 0–5 resolved from the registry or default heuristic.
    pub source_tier: u8,
    pub evidence_kinds: Vec<String>,
    /// Tier-0 primary/deterministic source.
    pub primary_source_bonus: bool,
    /// 0–1 against the source's `freshness_sla_days`; `None` when the hit
    /// carries no usable date (unscored, never a fabricated 1.0).
    pub recency_score: Option<f64>,
    /// 0–1: how much real article body the readability pass recovered vs a
    /// thin paywall / JS stub. Filled by the fetch layer.
    pub extraction_quality: f64,
    /// The paywall / JS-stub flag: extraction recovered too little body to
    /// treat the fetch as the page's content.
    pub thin_stub: bool,
}

/// Lowercase a host, strip a trailing dot and a leading `www.`, so lookups
/// and dedup key on the registrable-ish suffix the seed table uses.
pub fn normalize_host(host: &str) -> String {
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    h.strip_prefix("www.").unwrap_or(&h).to_string()
}

/// True when `host` is `domain` or a subdomain of it (both normalized).
fn matches_domain(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

/// One seed row: the registered domain suffix and its entry.
struct Seed {
    domain: &'static str,
    entry: RegistryEntry,
}

const fn seed(
    domain: &'static str,
    tier: u8,
    evidence_kinds: &'static [&'static str],
    lane_policy: LanePolicy,
    freshness_sla_days: u32,
    extraction_profile: ExtractionProfile,
    paywall: bool,
) -> Seed {
    Seed {
        domain,
        entry: RegistryEntry {
            tier,
            evidence_kinds,
            lane_policy,
            credential_mode: CredentialMode::None,
            freshness_sla_days,
            extraction_profile,
            paywall,
        },
    }
}

/// The representative starter set (`docs/data-sources.md §Source registry and
/// evidence tiers` — the tier examples), deliberately not a catalog of the
/// whole web: an unregistered domain takes the default heuristic below.
/// Freshness SLAs are drafted (long for filings/structured, short for news).
static SEEDS: &[Seed] = &[
    // Tier 0 — primary / deterministic.
    seed("sec.gov", 0, &["filings", "financials", "legal-disclosure"], LanePolicy::Evidence, 365, ExtractionProfile::ApiOrHtml, false),
    seed("federalreserve.gov", 0, &["macro", "rates", "policy"], LanePolicy::Evidence, 180, ExtractionProfile::Html, false),
    seed("stlouisfed.org", 0, &["macro", "rates"], LanePolicy::Evidence, 180, ExtractionProfile::ApiOrHtml, false),
    seed("bls.gov", 0, &["macro", "labor"], LanePolicy::Evidence, 90, ExtractionProfile::ApiOrHtml, false),
    seed("bea.gov", 0, &["macro"], LanePolicy::Evidence, 90, ExtractionProfile::ApiOrHtml, false),
    seed("eia.gov", 0, &["energy", "commodities"], LanePolicy::Evidence, 60, ExtractionProfile::ApiOrHtml, false),
    seed("fda.gov", 0, &["regulatory", "biotech"], LanePolicy::Evidence, 180, ExtractionProfile::Html, false),
    seed("cftc.gov", 0, &["positioning", "regulatory"], LanePolicy::Evidence, 30, ExtractionProfile::ApiOrHtml, false),
    seed("finra.org", 0, &["short-interest", "regulatory"], LanePolicy::Evidence, 30, ExtractionProfile::ApiOrHtml, false),
    // Tier 1 — licensed structured providers.
    seed("financialmodelingprep.com", 1, &["financials", "prices"], LanePolicy::Evidence, 30, ExtractionProfile::ApiOrHtml, false),
    seed("schwab.com", 1, &["brokerage", "prices"], LanePolicy::Evidence, 30, ExtractionProfile::JsRequired, false),
    seed("morningstar.com", 1, &["funds", "fair-value", "moat"], LanePolicy::Evidence, 60, ExtractionProfile::JsRequired, true),
    // Tier 2 — high-trust factual reporting.
    seed("reuters.com", 2, &["event-verification", "markets"], LanePolicy::Evidence, 14, ExtractionProfile::Html, false),
    seed("apnews.com", 2, &["event-verification"], LanePolicy::Evidence, 14, ExtractionProfile::Html, false),
    seed("wsj.com", 2, &["markets", "companies"], LanePolicy::Evidence, 14, ExtractionProfile::Html, true),
    seed("ft.com", 2, &["markets", "macro"], LanePolicy::Evidence, 14, ExtractionProfile::Html, true),
    seed("bloomberg.com", 2, &["markets", "companies"], LanePolicy::Evidence, 14, ExtractionProfile::JsRequired, true),
    seed("barrons.com", 2, &["markets"], LanePolicy::Evidence, 14, ExtractionProfile::Html, true),
    seed("economist.com", 2, &["macro", "regime"], LanePolicy::Evidence, 30, ExtractionProfile::Html, true),
    // Tier 3 — specialist industry sources (high trust within their vertical).
    seed("semianalysis.com", 3, &["semis-supply-chain", "ai-infra"], LanePolicy::Evidence, 30, ExtractionProfile::Html, true),
    seed("techinsights.com", 3, &["semis-teardown"], LanePolicy::Evidence, 60, ExtractionProfile::Html, true),
    seed("trendforce.com", 3, &["semis-supply-chain", "asp"], LanePolicy::Evidence, 21, ExtractionProfile::Html, false),
    seed("digitimes.com", 3, &["semis-supply-chain"], LanePolicy::Evidence, 21, ExtractionProfile::Html, true),
    seed("statnews.com", 3, &["biotech"], LanePolicy::Evidence, 21, ExtractionProfile::Html, true),
    seed("endpts.com", 3, &["biotech"], LanePolicy::Evidence, 21, ExtractionProfile::Html, true),
    seed("fiercebiotech.com", 3, &["biotech"], LanePolicy::Evidence, 21, ExtractionProfile::Html, false),
    seed("spglobal.com", 3, &["commodities", "credit"], LanePolicy::Evidence, 30, ExtractionProfile::Html, false),
    seed("argusmedia.com", 3, &["commodities"], LanePolicy::Evidence, 21, ExtractionProfile::Html, true),
    seed("woodmac.com", 3, &["energy", "materials"], LanePolicy::Evidence, 30, ExtractionProfile::Html, true),
    seed("freightwaves.com", 3, &["logistics"], LanePolicy::Evidence, 14, ExtractionProfile::Html, false),
    seed("benchmarkminerals.com", 3, &["battery-materials"], LanePolicy::Evidence, 30, ExtractionProfile::Html, true),
    // Tier 4 — useful but opinion-heavy.
    seed("seekingalpha.com", 4, &["analysis-opinion", "transcripts"], LanePolicy::Evidence, 21, ExtractionProfile::Html, true),
    seed("fool.com", 4, &["analysis-opinion", "transcripts"], LanePolicy::Evidence, 21, ExtractionProfile::Html, false),
    seed("substack.com", 4, &["analysis-opinion"], LanePolicy::Evidence, 30, ExtractionProfile::Html, false),
    // Tier 5 — sentiment / noise only.
    seed("reddit.com", 5, &["sentiment"], LanePolicy::SentimentOnly, 7, ExtractionProfile::JsRequired, false),
    seed("x.com", 5, &["sentiment"], LanePolicy::SentimentOnly, 7, ExtractionProfile::JsRequired, false),
    seed("twitter.com", 5, &["sentiment"], LanePolicy::SentimentOnly, 7, ExtractionProfile::JsRequired, false),
    seed("stocktwits.com", 5, &["sentiment"], LanePolicy::SentimentOnly, 7, ExtractionProfile::JsRequired, false),
    seed("youtube.com", 5, &["sentiment"], LanePolicy::SentimentOnly, 14, ExtractionProfile::JsRequired, false),
];

/// The deny seed — the one categorical exclusion (AI-generated forecast /
/// quote pages whose whole output is algorithmic filler, not evidence).
/// Drafted and deliberately tiny: deny drops junk that isn't evidence at all,
/// and membership accrues from live-run evidence rather than guesswork.
static DENY: &[(&str, &str)] = &[
    ("stockinvest.us", "algorithmic forecast pages, not evidence"),
    ("walletinvestor.com", "algorithmic forecast pages, not evidence"),
];

/// Resolve a host to its registry policy: an explicit deny, a registered
/// entry (suffix match), or the default heuristic — a recognized `.gov` /
/// company-IR host defaults to Tier 0, anything else to the unknown-blog
/// Tier 4 — so the system degrades gracefully rather than depending on
/// registry completeness.
pub fn assess(host: &str) -> SourcePolicy {
    let host = normalize_host(host);
    for (domain, reason) in DENY {
        if matches_domain(&host, domain) {
            return SourcePolicy::Deny(reason);
        }
    }
    for row in SEEDS {
        if matches_domain(&host, row.domain) {
            return SourcePolicy::Graded(row.entry.clone());
        }
    }
    SourcePolicy::Graded(default_entry(&host))
}

/// The default heuristic for an unregistered host.
fn default_entry(host: &str) -> RegistryEntry {
    if host.ends_with(".gov") {
        return RegistryEntry {
            tier: 0,
            evidence_kinds: &["government-primary"],
            lane_policy: LanePolicy::Evidence,
            credential_mode: CredentialMode::None,
            freshness_sla_days: 180,
            extraction_profile: ExtractionProfile::Html,
            paywall: false,
        };
    }
    // Company investor-relations hosts are primary sources for their own
    // issuer (`ir.example.com` / `investors.example.com` / `investor.example.com`).
    let ir_host = host
        .split_once('.')
        .is_some_and(|(label, _)| matches!(label, "ir" | "investor" | "investors"));
    if ir_host {
        return RegistryEntry {
            tier: 0,
            evidence_kinds: &["company-ir"],
            lane_policy: LanePolicy::Evidence,
            credential_mode: CredentialMode::None,
            freshness_sla_days: 120,
            extraction_profile: ExtractionProfile::Html,
            paywall: false,
        };
    }
    RegistryEntry {
        tier: 4,
        evidence_kinds: &["unregistered"],
        lane_policy: LanePolicy::Evidence,
        credential_mode: CredentialMode::None,
        freshness_sla_days: 21,
        extraction_profile: ExtractionProfile::Html,
        paywall: false,
    }
}

/// The 0–1 recency read against a source's freshness SLA: full weight inside
/// the SLA, then a smooth `sla/age` decay — a weight, never a gate
/// (`docs/web-research.md §Source quality and evidence weighting`; claim
/// freshness as a *floor* question is job-owned and lives elsewhere). Drafted,
/// calibratable.
pub fn recency_score(age_days: f64, sla_days: u32) -> f64 {
    if !age_days.is_finite() || age_days < 0.0 {
        return 1.0; // A future-dated or unreadable age never penalizes.
    }
    let sla = f64::from(sla_days.max(1));
    if age_days <= sla {
        1.0
    } else {
        (sla / age_days).clamp(0.0, 1.0)
    }
}

/// Assemble the app-computed annotation for a fetched document: the registry
/// read plus the fetch layer's extraction measurements. `age_days` is the
/// document's age at annotation time when a publish/retrieval date exists.
pub fn annotate(
    host: &str,
    age_days: Option<f64>,
    extraction_quality: f64,
    thin_stub: bool,
) -> Option<SourceAnnotation> {
    match assess(host) {
        SourcePolicy::Deny(_) => None,
        SourcePolicy::Graded(entry) => Some(SourceAnnotation {
            source_tier: entry.tier,
            evidence_kinds: entry.evidence_kinds.iter().map(|k| k.to_string()).collect(),
            primary_source_bonus: entry.tier == 0,
            recency_score: age_days.map(|d| recency_score(d, entry.freshness_sla_days)),
            extraction_quality: extraction_quality.clamp(0.0, 1.0),
            thin_stub,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_host_strips_www_case_and_trailing_dot() {
        assert_eq!(normalize_host("WWW.SEC.GOV."), "sec.gov");
        assert_eq!(normalize_host("efts.sec.gov"), "efts.sec.gov");
        assert_eq!(normalize_host("  reuters.com "), "reuters.com");
    }

    fn tier_of(host: &str) -> u8 {
        match assess(host) {
            SourcePolicy::Graded(e) => e.tier,
            SourcePolicy::Deny(_) => panic!("{host} unexpectedly denied"),
        }
    }

    #[test]
    fn registered_domains_resolve_including_subdomains() {
        assert_eq!(tier_of("sec.gov"), 0);
        assert_eq!(tier_of("efts.sec.gov"), 0, "subdomain suffix-matches");
        assert_eq!(tier_of("www.reuters.com"), 2);
        assert_eq!(tier_of("semianalysis.com"), 3);
        assert_eq!(tier_of("seekingalpha.com"), 4);
        assert_eq!(tier_of("reddit.com"), 5);
    }

    #[test]
    fn tier5_reads_sentiment_only() {
        let SourcePolicy::Graded(e) = assess("stocktwits.com") else {
            panic!("graded expected");
        };
        assert_eq!(e.lane_policy, LanePolicy::SentimentOnly);
    }

    #[test]
    fn deny_is_categorical_and_annotates_to_none() {
        assert!(matches!(assess("stockinvest.us"), SourcePolicy::Deny(_)));
        assert!(matches!(assess("www.stockinvest.us"), SourcePolicy::Deny(_)));
        assert_eq!(annotate("stockinvest.us", None, 0.9, false), None);
    }

    #[test]
    fn unregistered_hosts_take_the_default_heuristic() {
        // A .gov host defaults high (recognized primary), an unknown blog low.
        assert_eq!(tier_of("treasury.gov"), 0);
        assert_eq!(tier_of("randomstockblog.example"), 4);
        // Company IR hosts read primary for their own issuer.
        assert_eq!(tier_of("ir.tesla.com"), 0);
        assert_eq!(tier_of("investors.broadcom.com"), 0);
        // A host merely containing "ir" in a label does not.
        assert_eq!(tier_of("firstrepublic.example"), 4);
    }

    #[test]
    fn a_registered_suffix_never_matches_a_lookalike() {
        // `notsec.gov` matches the .gov heuristic (tier 0) but must not match
        // the sec.gov row's evidence kinds.
        let SourcePolicy::Graded(e) = assess("notsec.gov") else {
            panic!("graded expected");
        };
        assert_eq!(e.evidence_kinds, &["government-primary"]);
        // And a non-gov lookalike falls straight to the default.
        assert_eq!(tier_of("sec-gov.example"), 4);
    }

    #[test]
    fn recency_score_is_full_inside_the_sla_and_decays_past_it() {
        assert_eq!(recency_score(0.0, 14), 1.0);
        assert_eq!(recency_score(14.0, 14), 1.0);
        let decayed = recency_score(28.0, 14);
        assert!((decayed - 0.5).abs() < 1e-9, "sla/age decay: {decayed}");
        assert!(recency_score(1400.0, 14) < 0.02);
        // Unreadable / future ages never penalize.
        assert_eq!(recency_score(-3.0, 14), 1.0);
        assert_eq!(recency_score(f64::NAN, 14), 1.0);
    }

    #[test]
    fn annotate_assembles_the_app_computed_half() {
        let a = annotate("efts.sec.gov", Some(10.0), 0.83, false).unwrap();
        assert_eq!(a.source_tier, 0);
        assert!(a.primary_source_bonus);
        assert_eq!(a.recency_score, Some(1.0));
        assert!((a.extraction_quality - 0.83).abs() < 1e-9);
        assert!(!a.thin_stub);

        // No date -> no recency read (never a fabricated 1.0 with a date).
        let b = annotate("reuters.com", None, 0.4, true).unwrap();
        assert_eq!(b.recency_score, None);
        assert!(b.thin_stub);
        assert!(!b.primary_source_bonus);

        // Extraction quality is clamped to 0–1.
        let c = annotate("reuters.com", None, 7.0, false).unwrap();
        assert_eq!(c.extraction_quality, 1.0);
    }
}
