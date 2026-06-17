//! The analyst skills library — a shared catalog of reusable analytical lenses
//! supplied to the main agent by **progressive disclosure** (`docs/analyst-skills.md`).
//!
//! The mechanism is two-phase (`model_agent`): the agent first sees only each skill's
//! **frontmatter** ([`frontmatter_catalog`] — name + one-line description), requests the
//! subset relevant to this week's packet, and the application layer supplies the chosen
//! skills' full prompt **bodies** ([`select_bodies`]) into the generation prompt. Only
//! the requested subset is applied per report.
//!
//! This module is the app-layer data the disclosure rests on: the [`CATALOG`] *is* the
//! library, so the catalog the model selects from is exactly the set of authored skills.
//! Three deliberate, recorded narrowings from the doc hold for this slice:
//! - **Consumers:** only the main agent (during synthesis) receives skills, not the three
//!   Bull/Bear/Balanced analysts the doc also names.
//! - **Content:** 3 of the doc's 16 skills are authored here; the rest are a content
//!   follow-on that only adds [`CATALOG`] entries — the mechanism is complete.
//! - **Output:** skills *steer the synthesis prose* (each `body` is a lens folded into
//!   the unified thesis), so the doc's per-skill *output schema* is not built as a
//!   separate output channel.

/// One analytical skill. The `name` + `description` are the frontmatter the agent sees
/// during selection (phase 1); the `body` is the full prompt the application layer
/// supplies once the skill is requested (phase 2). The descriptions are taken verbatim
/// from `docs/analyst-skills.md` so the catalog matches the doc's frontmatter.
#[derive(Debug)]
pub struct Skill {
    pub name: &'static str,
    pub description: &'static str,
    pub body: &'static str,
}

/// The authored skills available this slice. Each `body` grounds its lens in baseline
/// data the Step-3 scan already gathers, and instructs steer-prose application (fold the
/// conclusion into the unified thesis, not a separate section). Extending the library is
/// purely additive — a follow-on appends entries here and they appear in the catalog and
/// the selection enum automatically.
pub const CATALOG: &[Skill] = &[
    Skill {
        name: "Market Regime Analysis",
        description: "Determines the current market regime and the dominant forces driving market behavior.",
        body: "Market Regime Analysis. Determine the regime the market is in and the dominant \
force driving it. Read the baseline indices, internals, sector performance, and the change view \
together: judge whether conditions are risk-on or risk-off, liquidity-driven or earnings-driven, \
inflation-sensitive or growth-sensitive, and whether leadership is broadening or narrowing. Weigh \
the financial-conditions indices (NFCI / ANFCI / STLFSI4), the curve and credit spreads, and \
breadth (movers, sector dispersion) as regime evidence rather than isolated prints. State the \
regime plainly, name the one or two forces that define it, and flag the development that would \
mark a regime change. Fold this read into the thesis as the frame the rest of the report sits \
inside — do not append it as a separate section.",
    },
    Skill {
        name: "Credit Stress Analysis",
        description: "Evaluates financial stress inside credit markets and identifies signs of tightening financial conditions.",
        body: "Credit Stress Analysis. Assess stress inside credit markets and whether financial \
conditions are tightening. Read the high-yield and investment-grade OAS, the BBB and single-B \
buckets (credit-quality dispersion), the 10y-3m and 10y-2y curve spreads, and the \
financial-conditions indices in the baseline and its change view. Judge whether spreads are \
widening or compressing, whether quality dispersion is rising (a risk-off tell), and whether a \
move is corroborated by the curve and conditions indices or is an isolated wobble — distinguish a \
genuine tightening from spread noise. Name the level and direction, what it implies for \
refinancing and liquidity risk, and the spread move that would change the read. Carry the \
conclusion into the thesis as a risk qualifier, not a standalone section.",
    },
    Skill {
        name: "Market Breadth Analysis",
        description: "Evaluates the health and participation level of the broader market beyond headline index performance.",
        body: "Market Breadth Analysis. Evaluate participation beneath the headline indices. Read \
the internals, sector performance, and the equity-breadth movers (gainers / losers / \
most-actives) against the index moves: judge whether a rally or selloff is broad-based or narrow, \
whether leadership is concentrated, and how the NASDAQ growth read compares with the broader NYSE \
read. Treat a divergence between index level and breadth as a signal the index alone hides. State \
whether breadth confirms or contradicts the index trend, name the concentration risk if \
leadership is narrow, and flag the breadth shift that would confirm or break the current move. \
Fold this into the thesis as evidence on the durability of the index trend, not a separate \
section.",
    },
];

/// The catalog's skill names, in catalog order — the source for the selection schema's
/// `enum`, so the model can only request a skill that exists (and is authored).
pub fn catalog_names() -> Vec<&'static str> {
    CATALOG.iter().map(|s| s.name).collect()
}

/// Render the frontmatter the agent sees during selection (phase 1): one `name:
/// description` line per skill. This is the cheap catalog the disclosure shows before any
/// full body is supplied.
pub fn frontmatter_catalog() -> String {
    CATALOG
        .iter()
        .map(|s| format!("- {}: {}", s.name, s.description))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Resolve the model's requested skill names to their full skill bodies (phase 2).
/// Iterating the catalog (not the request) makes the result **catalog-ordered,
/// deduplicated, and free of unknown names** in one pass — a defensive filter, though the
/// selection schema's `enum` already constrains the model to catalog names.
pub fn select_bodies(names: &[String]) -> Vec<&'static Skill> {
    CATALOG
        .iter()
        .filter(|s| names.iter().any(|n| n == s.name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_entries_are_well_formed_and_uniquely_named() {
        assert!(!CATALOG.is_empty(), "the catalog must carry at least one skill");
        for s in CATALOG {
            assert!(!s.name.trim().is_empty(), "a skill has a blank name");
            assert!(!s.description.trim().is_empty(), "{} has a blank description", s.name);
            assert!(!s.body.trim().is_empty(), "{} has a blank body", s.name);
        }
        let names = catalog_names();
        for (i, n) in names.iter().enumerate() {
            assert!(
                !names[i + 1..].contains(n),
                "duplicate skill name in the catalog: {n}"
            );
        }
    }

    #[test]
    fn frontmatter_catalog_lists_every_skill() {
        let catalog = frontmatter_catalog();
        for s in CATALOG {
            assert!(catalog.contains(s.name), "frontmatter missing name {}", s.name);
            assert!(
                catalog.contains(s.description),
                "frontmatter missing description for {}",
                s.name
            );
        }
        // The body must NOT leak into the frontmatter — disclosure shows only name +
        // description until a skill is requested.
        assert!(
            !catalog.contains(CATALOG[0].body),
            "frontmatter leaked a full skill body"
        );
    }

    #[test]
    fn select_bodies_resolves_known_names_in_catalog_order() {
        // Request the first two by name, out of catalog order; result is catalog-ordered.
        let want = vec![CATALOG[1].name.to_string(), CATALOG[0].name.to_string()];
        let got = select_bodies(&want);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, CATALOG[0].name, "result is catalog-ordered");
        assert_eq!(got[1].name, CATALOG[1].name);
    }

    #[test]
    fn select_bodies_drops_unknown_names_and_dedups() {
        let req = vec![
            CATALOG[0].name.to_string(),
            CATALOG[0].name.to_string(), // duplicate
            "No Such Skill".to_string(), // unknown
        ];
        let got = select_bodies(&req);
        assert_eq!(got.len(), 1, "unknown dropped, duplicate collapsed");
        assert_eq!(got[0].name, CATALOG[0].name);
    }

    #[test]
    fn select_bodies_is_empty_for_no_request() {
        assert!(select_bodies(&[]).is_empty());
    }
}
