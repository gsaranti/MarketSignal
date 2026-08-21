//! The loop-time listing-resolution guard (`docs/portfolio-analysis.md` §Asset
//! eligibility): Schwab's asset type says *equity*, not *US-listed equity*, while
//! the suite's data plan is US-only — so a stock's Schwab instrument identity must
//! resolve to a canonical FMP symbol whose profile identity cross-checks against
//! Schwab's before the pipeline may grade it. A wrong-issuer FMP mapping (an OTC
//! ticker collision) would otherwise grade the wrong company's financials, invisibly
//! to the run's own checks. The rules here are pure over an already-fetched profile
//! lookup; the fetch lives in the job (one fail-soft call per fresh-passed stock,
//! shared with the outcome episodes' entry-stamped sector identity).

/// The identity fields read off one FMP `/profile` body — the guard's cross-check
/// surface plus the sector label, one fetch feeding both consumers. Blank or absent
/// fields are `None` (the guard types them unverifiable, never a mismatch).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProfileIdentity {
    pub company_name: Option<String>,
    pub exchange: Option<String>,
    pub sector: Option<String>,
    /// The FMP industry label — the commodity context's gold-linkage key
    /// (an industry naming gold / precious metals, not the whole Basic
    /// Materials sector — `docs/data-sources.md §Portfolio Analysis —
    /// endpoint surface`, the `GCUSD` row).
    pub industry: Option<String>,
}

/// One `/profile` lookup outcome. `Unresolved` is only the definitive 2xx
/// empty-body shape — FMP answered and knows no such symbol; any gate, transport
/// failure, or unreadable body is `Unverified`, so an FMP outage can never read as
/// "this listing does not exist".
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileLookup {
    Resolved(ProfileIdentity),
    Unresolved,
    Unverified(String),
}

/// The guard's routing outcome for one stock, computed at gather time and routed by
/// `analyze_holding` beside the asset-class and net-short gates.
#[derive(Debug, Clone, PartialEq)]
pub enum ListingResolution {
    /// A resolved, matching US listing (US-listed ADRs included) — the full
    /// pipeline runs.
    SupportedUs,
    /// No canonical FMP resolution — not-rated (unsupported listing), a structural
    /// can't-grade.
    Unresolved,
    /// Resolved, but the primary listing is outside the US exchange set (a foreign
    /// ordinary, an OTC/PNK quote) — not-rated (unsupported listing).
    NonUs { exchange: String },
    /// Resolved on a US exchange, but the issuer identity conflicts with Schwab's —
    /// the evidence floor's conflicting-identity arm (`insufficient-evidence`,
    /// possibly transient).
    Conflict { fmp_name: String },
    /// The guard could not verify (profile gap, or a resolved profile whose
    /// exchange / name is unreadable, or a name comparison with nothing to compare)
    /// — the holding proceeds with a recorded degraded input.
    Unverified { detail: String },
}

/// The exchanges the guard accepts as a US primary listing. Deliberately excludes
/// OTC/PNK: a foreign ordinary or unsponsored ADR quotes there without SEC filings,
/// and that venue is where ticker collisions concentrate — the cost (a US microcap
/// quoted only OTC is never graded) is the ruled trade (2026-08-05).
const US_EXCHANGES: [&str; 3] = ["NYSE", "NASDAQ", "AMEX"];

/// Corporate-form and share-class tokens that carry no issuer identity — dropped
/// before the name comparison so "Apple Inc" and "APPLE INC COM" compare on
/// {APPLE} alone. Single-character tokens (share-class letters) are dropped by
/// length. Drafted with the slice; extend as real Schwab descriptions surface.
const NAME_STOPWORDS: [&str; 34] = [
    "INC",
    "INCORPORATED",
    "CORP",
    "CORPORATION",
    "CO",
    "COMPANY",
    "LTD",
    "LIMITED",
    "PLC",
    "LP",
    "LLC",
    "HOLDINGS",
    "HOLDING",
    "HLDGS",
    "HLDG",
    "GROUP",
    "GRP",
    "ADR",
    "ADS",
    "SPONSORED",
    "SPON",
    "COM",
    "COMMON",
    "STOCK",
    "SHS",
    "SHARES",
    "SHARE",
    "CL",
    "CLASS",
    "ORD",
    "ORDINARY",
    "NEW",
    "THE",
    "NV",
];

/// Uppercased identity-bearing tokens of an issuer name: split on
/// non-alphanumerics, corporate-form stopwords and single-character tokens dropped.
fn significant_tokens(name: &str) -> Vec<String> {
    name.to_ascii_uppercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() > 1 && !NAME_STOPWORDS.contains(t))
        .map(String::from)
        .collect()
}

/// The conservative comparison: conflict **only** when the two names share zero
/// significant tokens (the gross OTC-collision shape); a shared token is a match
/// (so "American Airlines" vs "American Express" deliberately does not conflict —
/// no spurious abstention), and an empty token set on either side is unverifiable,
/// never a conflict.
enum NameComparison {
    Match,
    Conflict,
    Unverifiable,
}

fn compare_names(schwab: &str, fmp: &str) -> NameComparison {
    let a = significant_tokens(schwab);
    let b = significant_tokens(fmp);
    if a.is_empty() || b.is_empty() {
        return NameComparison::Unverifiable;
    }
    if a.iter().any(|t| b.contains(t)) {
        NameComparison::Match
    } else {
        NameComparison::Conflict
    }
}

/// How much identity an account description carries, relative to the ticker.
enum DescriptionIdentity {
    /// At least one significant token beyond the ticker's own — a real name.
    Issuer,
    /// Significant tokens, but every one is a ticker token: the ticker
    /// repeated, or a ticker-plus-stopword shape like `"PSX COM"`. Weak
    /// evidence — some issuers ARE named by their ticker (ASML), so the
    /// guard tests this shape against the profile name rather than comparing
    /// it as a name or skipping it outright. Token-set comparison (not raw
    /// string equality) keeps a slash-class self-description (`"BRK/B"` for
    /// `BRK/B`) in this family too.
    TickerOnly,
    /// Nothing significant at all — blank, whitespace, or corporate-form
    /// noise like `"COMMON STOCK"` that tokenizes to nothing.
    None,
}

fn description_identity(schwab_description: &str, symbol: &str) -> DescriptionIdentity {
    let tokens = significant_tokens(schwab_description);
    if tokens.is_empty() {
        return DescriptionIdentity::None;
    }
    let ticker_tokens = significant_tokens(symbol);
    if tokens.iter().any(|t| !ticker_tokens.contains(t)) {
        DescriptionIdentity::Issuer
    } else {
        DescriptionIdentity::TickerOnly
    }
}

/// Whether an account description carries an issuer identity at all — the one
/// definition, shared by the guard below and the interpretation prompt's issuer
/// fallback so the two cannot drift apart. Identity is a significant token
/// **beyond the ticker's own tokens** ([`DescriptionIdentity::Issuer`]), not
/// merely non-emptiness: blank, noise, the ticker repeated, and the
/// ticker-plus-stopword shape all name no issuer for a prompt header to show.
pub fn describes_issuer(schwab_description: &str, symbol: &str) -> bool {
    matches!(
        description_identity(schwab_description, symbol),
        DescriptionIdentity::Issuer
    )
}

/// Whether a **canonical-source** name (the FMP profile's `companyName`, the
/// fund data's `name`) is displayable for `symbol`. Deliberately looser than
/// [`describes_issuer`]: a canonical source's name field is an issuer name by
/// construction, so a ticker-token-only *legal* name — "ASML Holding N.V.",
/// "eBay Inc." — is real identity there, while the same token shape in an
/// account description ("PSX COM") is noise. Only emptiness-after-tokenizing
/// and the bare ticker are rejected (FMP's parser accepts any non-blank
/// string, so those shapes do reach the fallback).
pub fn displayable_source_name(name: &str, symbol: &str) -> bool {
    let n = name.trim();
    !n.eq_ignore_ascii_case(symbol.trim()) && !significant_tokens(n).is_empty()
}

/// Route one stock's profile lookup to its listing resolution. The exchange test
/// runs before the name comparison — the exchange, not the profile's HQ country,
/// is what lets a US-listed ADR pass — and a Schwab description carrying no issuer
/// identity ([`describes_issuer`]) has nothing to compare, so it reads unverifiable.
/// Blank and noise-only descriptions reached the same `Unverified` outcome through
/// `compare_names`'s empty-token arm before that check was hoisted; only the
/// recorded detail changed. That arm stays live for the mirror case — a *profile*
/// name that tokenizes to nothing.
pub fn resolve_listing(
    symbol: &str,
    schwab_description: &str,
    lookup: &ProfileLookup,
) -> ListingResolution {
    match lookup {
        ProfileLookup::Unresolved => ListingResolution::Unresolved,
        ProfileLookup::Unverified(detail) => ListingResolution::Unverified {
            detail: detail.clone(),
        },
        ProfileLookup::Resolved(profile) => {
            let Some(exchange) = profile.exchange.as_deref() else {
                return ListingResolution::Unverified {
                    detail: "resolved profile carried no exchange".to_string(),
                };
            };
            if !US_EXCHANGES.iter().any(|us| exchange.eq_ignore_ascii_case(us)) {
                return ListingResolution::NonUs {
                    exchange: exchange.to_string(),
                };
            }
            let Some(fmp_name) = profile.company_name.as_deref() else {
                return ListingResolution::Unverified {
                    detail: "resolved profile carried no issuer name".to_string(),
                };
            };
            match description_identity(schwab_description, symbol) {
                DescriptionIdentity::None => ListingResolution::Unverified {
                    detail: "account description carries no issuer name to cross-check"
                        .to_string(),
                },
                // The ticker is the description's only identity token. Some
                // issuers are named by their ticker (ASML), so test the ticker
                // itself against the profile name through the one shared
                // comparison: a match verifies, and everything else reads
                // unverifiable — deliberately never a conflict, since a
                // "PSX COM" shape names no issuer to conflict with (attempt-1
                // review sweep). TickerOnly guarantees non-empty ticker
                // tokens, so the Unverifiable arm folds in harmlessly.
                DescriptionIdentity::TickerOnly => match compare_names(symbol, fmp_name) {
                    NameComparison::Match => ListingResolution::SupportedUs,
                    NameComparison::Conflict | NameComparison::Unverifiable => {
                        ListingResolution::Unverified {
                            detail: "account description carries only the ticker, which \
                                     the resolved issuer name does not contain"
                                .to_string(),
                        }
                    }
                },
                DescriptionIdentity::Issuer => match compare_names(schwab_description, fmp_name) {
                    NameComparison::Match => ListingResolution::SupportedUs,
                    NameComparison::Conflict => ListingResolution::Conflict {
                        fmp_name: fmp_name.to_string(),
                    },
                    NameComparison::Unverifiable => ListingResolution::Unverified {
                        detail: "issuer names carried no comparable tokens".to_string(),
                    },
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, exchange: &str) -> ProfileLookup {
        ProfileLookup::Resolved(ProfileIdentity {
            company_name: Some(name.to_string()),
            exchange: Some(exchange.to_string()),
            sector: Some("Technology".to_string()),
            industry: None,
        })
    }

    #[test]
    fn a_matching_us_listing_is_supported_despite_suffix_noise() {
        // The plan's named pair: exact match is too strict; the suffix tokens carry
        // no identity.
        assert_eq!(
            resolve_listing("AAPL", "APPLE INC COM", &profile("Apple Inc.", "NASDAQ")),
            ListingResolution::SupportedUs
        );
        assert_eq!(
            resolve_listing("NVDA", "NVIDIA CORP", &profile("NVIDIA Corporation", "NASDAQ")),
            ListingResolution::SupportedUs
        );
        // A US-listed ADR passes on the exchange test, never an HQ-country read.
        assert_eq!(
            resolve_listing(
                "ASML",
                "ASML HOLDING NV ADR",
                &profile("ASML Holding N.V.", "NASDAQ")
            ),
            ListingResolution::SupportedUs
        );
    }

    #[test]
    fn the_exchange_test_is_case_insensitive_across_the_allowlist() {
        for exchange in ["NYSE", "nasdaq", "Amex"] {
            assert_eq!(
                resolve_listing("ACME", "ACME WIDGETS INC", &profile("Acme Widgets Inc", exchange)),
                ListingResolution::SupportedUs,
                "{exchange} should count as a US listing"
            );
        }
    }

    #[test]
    fn a_non_us_or_otc_primary_listing_is_unsupported() {
        // A foreign ordinary quoted OTC (the doc's example case) and a foreign
        // exchange both fall outside the allowlist — even with a matching name.
        for exchange in ["PNK", "OTC", "LSE", "XETRA"] {
            assert_eq!(
                resolve_listing(
                    "NTDOF",
                    "NINTENDO CO LTD",
                    &profile("Nintendo Co., Ltd.", exchange)
                ),
                ListingResolution::NonUs {
                    exchange: exchange.to_string()
                },
            );
        }
    }

    #[test]
    fn no_resolution_routes_unresolved_and_a_gap_routes_unverified() {
        assert_eq!(
            resolve_listing("ZZZQ", "SOME COMPANY INC", &ProfileLookup::Unresolved),
            ListingResolution::Unresolved
        );
        // A transport / plan gap is never mistaken for a missing listing.
        assert_eq!(
            resolve_listing(
                "AAPL",
                "APPLE INC",
                &ProfileLookup::Unverified("FMP profile unavailable (unavailable)".to_string())
            ),
            ListingResolution::Unverified {
                detail: "FMP profile unavailable (unavailable)".to_string()
            }
        );
    }

    #[test]
    fn a_zero_shared_token_name_is_a_conflict_and_one_shared_token_is_not() {
        // The gross OTC-collision shape: nothing in common.
        assert_eq!(
            resolve_listing(
                "ACME",
                "ACME PHARMACEUTICALS INC",
                &profile("Zenith Mining Corp", "NYSE")
            ),
            ListingResolution::Conflict {
                fmp_name: "Zenith Mining Corp".to_string()
            }
        );
        // The ruled conservative bound, pinned: one shared significant token is a
        // match — no spurious abstention on near-collisions.
        assert_eq!(
            resolve_listing(
                "AAL",
                "AMERICAN AIRLINES GROUP INC",
                &profile("American Express Company", "NYSE")
            ),
            ListingResolution::SupportedUs
        );
    }

    #[test]
    fn nothing_to_compare_reads_unverified_never_conflict() {
        // A ticker-only account description has no issuer identity to cross-check.
        let r = resolve_listing("NTDOF", "NTDOF", &profile("Nintendo Co., Ltd.", "NYSE"));
        assert!(matches!(r, ListingResolution::Unverified { .. }), "{r:?}");
        // A description that is all corporate-form noise tokenizes to nothing.
        let r = resolve_listing("XYZ", "COMMON STOCK", &profile("Xyz Industries Inc", "NYSE"));
        assert!(matches!(r, ListingResolution::Unverified { .. }), "{r:?}");
        // Ticker-plus-stopword — the real account-description shape "PSX COM"
        // tokenizes back to just the ticker, naming no issuer: it must read
        // unverifiable, never make a good listing guard-terminal by comparing
        // {PSX} against {PHILLIPS, 66} (attempt-1 review sweep).
        let r = resolve_listing("PSX", "PSX COM", &profile("Phillips 66", "NYSE"));
        assert!(matches!(r, ListingResolution::Unverified { .. }), "{r:?}");
        // A resolved profile missing its exchange or name cannot be verified —
        // and is never routed terminal.
        let no_exchange = ProfileLookup::Resolved(ProfileIdentity {
            company_name: Some("Apple Inc.".to_string()),
            exchange: None,
            sector: None,
            industry: None,
        });
        assert!(matches!(
            resolve_listing("AAPL", "APPLE INC", &no_exchange),
            ListingResolution::Unverified { .. }
        ));
        let no_name = ProfileLookup::Resolved(ProfileIdentity {
            company_name: None,
            exchange: Some("NASDAQ".to_string()),
            sector: None,
            industry: None,
        });
        assert!(matches!(
            resolve_listing("AAPL", "APPLE INC", &no_name),
            ListingResolution::Unverified { .. }
        ));
    }

    #[test]
    fn describes_issuer_demands_a_token_beyond_the_ticker() {
        // Identity-bearing shapes.
        assert!(describes_issuer("PHILLIPS 66", "PSX"));
        assert!(describes_issuer("Apple Inc.", "AAPL"));
        // No-identity shapes: blank, noise, the ticker itself, and the
        // ticker-plus-stopword account form.
        assert!(!describes_issuer("", "PSX"));
        assert!(!describes_issuer("COMMON STOCK", "PSX"));
        assert!(!describes_issuer("PSX", "PSX"));
        assert!(!describes_issuer("PSX COM", "PSX"));
        assert!(!describes_issuer("psx common stock", "PSX"));
        // A slash-class symbol's self-description compares by token set, not
        // raw string equality.
        assert!(!describes_issuer("BRK/B", "BRK/B"));
        assert!(describes_issuer("BERKSHIRE HATHAWAY INC", "BRK/B"));
    }
}
