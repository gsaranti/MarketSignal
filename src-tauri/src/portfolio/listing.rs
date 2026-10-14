//! The listing-resolution guard (`docs/portfolio-analysis.md` §Asset
//! eligibility): a stock's Schwab identity must resolve to a canonical FMP
//! symbol whose profile cross-checks against Schwab's before the pipeline may
//! grade it, so an OTC ticker collision never grades the wrong issuer. The rules
//! are pure over an already-fetched profile lookup; the fetch lives in the job.

/// The identity fields read off one FMP `/profile` body. Blank or absent fields
/// are `None`, which the guard types unverifiable, never a mismatch.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProfileIdentity {
    /// The quote denomination, independent of the listing venue and the
    /// financial-statement denomination.
    pub currency: Option<String>,
    pub is_adr: Option<bool>,
    pub company_name: Option<String>,
    /// An IR eligibility hint, never evidence that two documents are equivalent.
    pub website: Option<String>,
    pub exchange: Option<String>,
    pub sector: Option<String>,
    /// The FMP industry label — the commodity context's gold-linkage key
    /// (`docs/data-sources.md §Portfolio Analysis — endpoint surface`, the
    /// `GCUSD` row).
    pub industry: Option<String>,
    /// The provider's served beta (`portfolio::factors`,
    /// `portfolio::value_creation`); `None` where the profile omits it or serves
    /// a non-numeric value.
    pub beta: Option<f64>,
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

#[derive(Debug, Clone, PartialEq)]
pub enum ListingResolution {
    /// A resolved, matching US listing with no unsupported unit signal.
    SupportedUs,
    /// No verified mapping from provider amounts to the listed USD share.
    UnsupportedUnits { detail: String },
    /// No canonical FMP resolution — not-rated (unsupported listing).
    Unresolved,
    /// Resolved, but the primary listing is outside the US exchange set — not-rated
    /// (unsupported listing).
    NonUs { exchange: String },
    /// Resolved on a US exchange, but the issuer identity conflicts with Schwab's —
    /// the evidence floor's conflicting-identity arm (`insufficient-evidence`).
    Conflict { fmp_name: String },
    /// The guard could not verify; the holding proceeds with a recorded degraded
    /// input.
    Unverified { detail: String },
}

/// Deliberately excludes OTC/PNK: a foreign ordinary or unsponsored ADR quotes
/// there without SEC filings, and that venue is where ticker collisions
/// concentrate; the cost is that a US microcap quoted only OTC is never graded.
const US_EXCHANGES: [&str; 3] = ["NYSE", "NASDAQ", "AMEX"];

/// Corporate-form and share-class tokens that carry no issuer identity, dropped
/// before the name comparison so "Apple Inc" and "APPLE INC COM" compare on
/// {APPLE} alone. Single-character tokens (share-class letters) are dropped by
/// length. Extend as real Schwab descriptions surface.
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

/// Uppercased identity-bearing tokens of an issuer name. Apostrophes collapse
/// inside the word (`McDonald's` → `MCDONALDS`) before the split: that is
/// faithful to account descriptions that omit them and more conservative across
/// unrelated names (`O'Reilly` contributes no loose `REILLY` token).
fn significant_tokens(name: &str) -> Vec<String> {
    let normalized: String = name
        .to_ascii_uppercase()
        .chars()
        .filter(|c| !matches!(c, '\'' | '’' | '‘' | 'ʼ'))
        .collect();
    normalized
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() > 1 && !NAME_STOPWORDS.contains(t))
        .map(String::from)
        .collect()
}

/// Conflict only when the two names share zero significant tokens (the gross
/// OTC-collision shape); one shared token is a match, so "American Airlines" vs
/// "American Express" deliberately does not conflict, and an empty token set on
/// either side is unverifiable, never a conflict.
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
    /// Significant tokens, but every one is a ticker token: the ticker repeated,
    /// or a ticker-plus-stopword shape like `"PSX COM"`. Some issuers are named
    /// by their ticker (ASML), so the guard tests this shape against the profile
    /// name rather than comparing it as a name or skipping it. Token-set
    /// comparison keeps a slash-class self-description (`"BRK/B"`) here too.
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

/// Whether an account description carries an issuer identity: a significant
/// token beyond the ticker's own ([`DescriptionIdentity::Issuer`]), not mere
/// non-emptiness. The one definition, shared by the guard and the interpretation
/// prompt's issuer fallback so the two cannot drift apart.
pub fn describes_issuer(schwab_description: &str, symbol: &str) -> bool {
    matches!(
        description_identity(schwab_description, symbol),
        DescriptionIdentity::Issuer
    )
}

/// Whether a canonical-source name (the FMP profile's `companyName`, the fund
/// data's `name`) is displayable for `symbol`. Deliberately looser than
/// [`describes_issuer`]: a canonical source's name field is an issuer name by
/// construction, so a ticker-token-only legal name ("ASML Holding N.V.") is
/// real identity there. Only emptiness-after-tokenizing and the bare ticker are
/// rejected, since FMP's parser accepts any non-blank string.
pub fn displayable_source_name(name: &str, symbol: &str) -> bool {
    let n = name.trim();
    !n.eq_ignore_ascii_case(symbol.trim()) && !significant_tokens(n).is_empty()
}

/// Route one stock's profile lookup to its listing resolution. The exchange test
/// runs before the name comparison and is independent of headquarters country.
/// A description carrying no issuer identity ([`describes_issuer`]) has nothing
/// to compare, so it reads unverifiable; `compare_names`'s empty-token arm
/// covers the mirror case, a profile name that tokenizes to nothing.
pub fn resolve_listing(
    symbol: &str,
    schwab_description: &str,
    lookup: &ProfileLookup,
) -> ListingResolution {
    let description_is_adr = schwab_description
        .to_ascii_uppercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| matches!(word, "ADR" | "ADS" | "DEPOSITARY" | "DEPOSITORY"));
    if description_is_adr || matches!(lookup, ProfileLookup::Resolved(p) if p.is_adr == Some(true))
    {
        return ListingResolution::UnsupportedUnits {
            detail: "depositary receipt — provider per-share amounts have no verified ordinary-share-to-ADS conversion".into(),
        };
    }
    if let ProfileLookup::Resolved(p) = lookup {
        if p.currency
            .as_deref()
            .is_some_and(|c| !c.eq_ignore_ascii_case("USD"))
        {
            return ListingResolution::UnsupportedUnits {
                detail:
                    "the listing quote is not denominated in USD — no FX conversion is available"
                        .into(),
            };
        }
    }
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
                // Some issuers are named by their ticker (ASML), so the ticker
                // itself is tested against the profile name: a match verifies,
                // anything else reads unverifiable — never a conflict, since a
                // "PSX COM" shape names no issuer to conflict with.
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
            website: None,
            currency: Some("USD".into()),
            is_adr: Some(false),
            company_name: Some(name.to_string()),
            exchange: Some(exchange.to_string()),
            sector: Some("Technology".to_string()),
            industry: None,
            beta: None,
        })
    }

    #[test]
    fn a_matching_us_listing_is_supported_despite_suffix_noise() {
        assert_eq!(
            resolve_listing("AAPL", "APPLE INC COM", &profile("Apple Inc.", "NASDAQ")),
            ListingResolution::SupportedUs
        );
        assert_eq!(
            resolve_listing(
                "NVDA",
                "NVIDIA CORP",
                &profile("NVIDIA Corporation", "NASDAQ")
            ),
            ListingResolution::SupportedUs
        );
        // A USD exchange quote does not establish the provider's ADS share basis.
        assert!(matches!(
            resolve_listing(
                "ASML",
                "ASML HOLDING NV ADR",
                &profile("ASML Holding N.V.", "NASDAQ")
            ),
            ListingResolution::UnsupportedUnits { .. }
        ));
        let mut adr = profile("Acme", "NYSE");
        if let ProfileLookup::Resolved(ref mut p) = adr {
            p.is_adr = Some(true);
        }
        assert!(matches!(
            resolve_listing("ACME", "ACME", &adr),
            ListingResolution::UnsupportedUnits { .. }
        ));
        if let ProfileLookup::Resolved(ref mut p) = adr {
            p.is_adr = Some(false);
            p.currency = Some("CAD".into());
        }
        assert!(matches!(
            resolve_listing("ACME", "ACME", &adr),
            ListingResolution::UnsupportedUnits { .. }
        ));
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
        // Outside the allowlist even with a matching name.
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
        // One shared token is a match: no spurious abstention on near-collisions.
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
    fn possessive_issuer_names_match_the_account_form_without_an_apostrophe() {
        for (symbol, schwab, canonical) in [
            ("MCD", "MCDONALDS CORP", "McDonald's Corporation"),
            ("KSS", "KOHLS CORP", "Kohl’s Corporation"),
            ("MCO", "MOODYS CORP", "Moody's Corporation"),
            ("WEN", "WENDYS CO", "Wendy’s Company"),
        ] {
            assert_eq!(
                resolve_listing(symbol, schwab, &profile(canonical, "NYSE")),
                ListingResolution::SupportedUs,
                "{symbol}: {schwab} vs {canonical}"
            );
            // Symmetric with respect to which source keeps the punctuation.
            assert!(
                matches!(compare_names(canonical, schwab), NameComparison::Match),
                "{symbol}: reverse comparison"
            );
        }

        // O'Reilly and an unrelated Reilly issuer remain a zero-token conflict.
        assert!(matches!(
            compare_names("O'Reilly Automotive", "Reilly Financial"),
            NameComparison::Conflict
        ));
        assert!(matches!(
            compare_names("Dick's Sporting Goods", "Dick Corporation"),
            NameComparison::Conflict
        ));
    }

    #[test]
    fn nothing_to_compare_reads_unverified_never_conflict() {
        // A ticker-only account description has no issuer identity to cross-check.
        let r = resolve_listing("NTDOF", "NTDOF", &profile("Nintendo Co., Ltd.", "NYSE"));
        assert!(matches!(r, ListingResolution::Unverified { .. }), "{r:?}");
        // A description that is all corporate-form noise tokenizes to nothing.
        let r = resolve_listing("XYZ", "COMMON STOCK", &profile("Xyz Industries Inc", "NYSE"));
        assert!(matches!(r, ListingResolution::Unverified { .. }), "{r:?}");
        // "PSX COM" tokenizes back to the ticker, so it must not go terminal by
        // comparing {PSX} against {PHILLIPS, 66}.
        let r = resolve_listing("PSX", "PSX COM", &profile("Phillips 66", "NYSE"));
        assert!(matches!(r, ListingResolution::Unverified { .. }), "{r:?}");
        // A resolved profile missing its exchange or name is never routed terminal.
        let no_exchange = ProfileLookup::Resolved(ProfileIdentity {
            website: None,
            currency: Some("USD".into()),
            is_adr: Some(false),
            company_name: Some("Apple Inc.".to_string()),
            exchange: None,
            sector: None,
            industry: None,
            beta: None,
        });
        assert!(matches!(
            resolve_listing("AAPL", "APPLE INC", &no_exchange),
            ListingResolution::Unverified { .. }
        ));
        let no_name = ProfileLookup::Resolved(ProfileIdentity {
            website: None,
            currency: Some("USD".into()),
            is_adr: Some(false),
            company_name: None,
            exchange: Some("NASDAQ".to_string()),
            sector: None,
            industry: None,
            beta: None,
        });
        assert!(matches!(
            resolve_listing("AAPL", "APPLE INC", &no_name),
            ListingResolution::Unverified { .. }
        ));
    }

    #[test]
    fn describes_issuer_demands_a_token_beyond_the_ticker() {
        assert!(describes_issuer("PHILLIPS 66", "PSX"));
        assert!(describes_issuer("Apple Inc.", "AAPL"));
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
