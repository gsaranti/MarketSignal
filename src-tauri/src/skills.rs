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
//! All 16 of the doc's skills are authored here. Two deliberate, recorded narrowings from
//! the doc still hold:
//! - **Consumers:** only the main agent (during synthesis) receives skills, not the three
//!   Bull/Bear/Balanced analysts the doc also names.
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
    Skill {
        name: "Narrative vs Reality",
        description: "Separates genuine market or economic changes from exaggerated media narratives and short-term emotional reactions.",
        body: "Narrative vs Reality. Separate genuine market and economic change from exaggerated \
media narratives and short-term emotional reactions. Take the dominant narratives surfaced in \
the research packet and test each against the baseline and its change view: do the indices, \
internals, breadth, credit spreads, financial-conditions indices, and the macro prints actually \
corroborate the story, or is the move headline-driven and unconfirmed by positioning and data? \
Name where the data backs the narrative and where it contradicts it, and call out a narrative \
running ahead of the evidence. Fold the verdict into the thesis as a confidence weight on each \
theme — do not append it as a separate section.",
    },
    Skill {
        name: "Second-Order Effects",
        description: "Analyzes downstream consequences of major market, economic, geopolitical, or policy developments.",
        body: "Second-Order Effects. Map the downstream consequences of the major market, economic, \
geopolitical, or policy developments this week surfaces. For each first-order move in the change \
view or the research packet, trace how it propagates into inflation (PCE, breakevens, expected \
inflation, oil and gas), yields and the curve (the 10-year yield, the 10y-3m and 10y-2y spreads), \
liquidity and financial conditions (NFCI / ANFCI / STLFSI4, credit spreads), sector performance, \
and consumer behavior (consumer sentiment, retail sales). Name the two or three second-order \
chains that matter most and the signal that would confirm each is playing out. Fold these into \
the thesis as the forward consequences the strategy must price — do not append them as a separate \
section.",
    },
    Skill {
        name: "Inflation Decomposition",
        description: "Breaks inflation into its underlying components and evaluates whether inflation pressure is temporary, structural, broadening, or narrowing.",
        body: "Inflation Decomposition. Break inflation into its components and judge whether the \
pressure is temporary or structural, broadening or narrowing. Read the gathered inflation signals \
together — PCE, PPI, the market-implied breakevens, the one-year expected-inflation gauge, energy \
via oil and natural gas, and wage pressure via the labor levels — and lean on the research packet \
for the shelter, services, and goods detail the baseline does not break out. Distinguish an \
energy- or goods-led pulse that should fade from a services- or wage-led pressure that tends to \
persist. Name which components are driving the print and whether the breadth is widening. Fold \
the read into the thesis as the inflation trajectory the rate and valuation views depend on — do \
not append it as a separate section.",
    },
    Skill {
        name: "Historical Analog",
        description: "Compares current market conditions to historical market environments and macroeconomic periods.",
        body: "Historical Analog. Compare current conditions to historical market and macro \
environments. Read the regime, valuations, credit, and the change view together, draw on vector \
memory and the recent reports for the thesis's own prior turns, and identify the closest analog — \
a dot-com-style valuation extreme, an inflationary or tightening cycle, a liquidity crisis, or a \
prior commodity or geopolitical shock. Be explicit about where the analog holds and where it \
breaks, so the comparison illuminates rather than forces a pattern. Name the one analog that \
frames the present best and the difference that most limits it. Fold this into the thesis as \
historical context for the central call — do not append it as a separate section.",
    },
    Skill {
        name: "Positioning & Sentiment",
        description: "Analyzes investor psychology, market positioning, and sentiment conditions.",
        body: "Positioning & Sentiment. Read investor psychology, positioning, and sentiment. Use \
the gathered tells — the volatility term structure (VIX against the 3-month VXV, and VXN for the \
Nasdaq), the equity-breadth movers and most-actives, sector dispersion, and consumer sentiment — \
alongside the positioning and flow color in the research packet to judge whether the market is \
fearful or greedy, complacent or capitulating, and where trades look crowded. Watch for euphoria, \
or for excessive pessimism that sets up an asymmetric reaction. Name the current sentiment state \
and the crowded trade most exposed to a reversal. Fold this into the thesis as a contrarian check \
on the central call — do not append it as a separate section.",
    },
    Skill {
        name: "Thesis Stress Test",
        description: "Challenges the current market thesis and searches for weak assumptions or contradictory evidence.",
        body: "Thesis Stress Test. Challenge the current thesis and hunt for its weak assumptions. \
Take the standing thesis from the recent reports and vector memory, then stress it against the \
change view, the data-gaps manifest, and any contradictory evidence in the baseline and research: \
name the assumptions that are most fragile, the signals the thesis is currently discounting, and \
what could invalidate it. State the specific conditions that would force a reassessment or a \
pivot. Be adversarial — the goal is to find where the thesis is wrong, not to reaffirm it. Fold \
the findings into the thesis as its stated risks and pivot triggers — do not append them as a \
separate section.",
    },
    Skill {
        name: "Geopolitical Escalation",
        description: "Evaluates geopolitical developments and their potential market implications.",
        body: "Geopolitical Escalation. Evaluate geopolitical developments and their market \
implications. The baseline carries no geopolitical feed, so work from the conflicts, trade \
tensions, sanctions, shipping disruptions, and supply-chain risks surfaced in the research packet, \
and corroborate their market read against the gathered safe-haven and commodity moves — oil and \
natural gas, gold, the dollar, and credit spreads. Judge whether a development is a contained \
headline or a genuine escalation with commodity and supply-chain transmission. Name the one or \
two risks that bear on the thesis and the market signal that would confirm escalation. Fold this \
into the thesis as a tail-risk qualifier — do not append it as a separate section.",
    },
    Skill {
        name: "AI Infrastructure Chain",
        description: "Analyzes the AI infrastructure ecosystem and its broader market implications.",
        body: "AI Infrastructure Chain. Analyze the AI infrastructure ecosystem and its market \
implications. Anchor on what the baseline gathers — technology and semiconductor sector \
performance, the industry-level P/E and rotation snapshots, and the power-demand link through the \
energy series — and draw the chain detail (semiconductors, datacenters, HBM memory, networking, \
optics, cooling, capital expenditure) from the research packet. Judge whether the buildout is \
broadening or concentrating, whether valuations are supported by the capex trend, and where power \
and energy constraints bind. Name the segment leading the chain and the signal that would mark a \
turn. Fold this into the thesis as the read on the market's dominant structural driver — do not \
append it as a separate section.",
    },
    Skill {
        name: "Time Horizon Separation",
        description: "Separates short-term market reactions from medium-term and long-term structural market trends.",
        body: "Time Horizon Separation. Separate short-term reactions from medium- and long-term \
structural trends. Use the cadence-honest change view to isolate this interval's moves, and weigh \
them against the recent reports and vector memory to tell weekly noise from cyclical developments \
from structural long-term shifts. Resist letting a single volatile print pull the thesis; resist \
dismissing a slow structural change as noise. Name what is genuinely transient this week and what \
is a durable shift the thesis should absorb. Fold this into the thesis as the calibration between \
near-term commentary and the standing long-term view — do not append it as a separate section.",
    },
    Skill {
        name: "Energy Security Analysis",
        description: "Analyzes energy-market stability and the macroeconomic implications of energy disruptions.",
        body: "Energy Security Analysis. Assess energy-market stability and the macro implications \
of energy disruptions. Read the gathered energy levels — oil (WTI) and natural gas, with gold and \
the dollar as corroboration — and the change view, and draw OPEC activity, shipping chokepoints, \
and grid-stress detail from the research packet. Judge whether an energy move is a supply shock \
with inflation and growth consequences or ordinary volatility, and weigh the link between \
AI-infrastructure power demand and energy prices. Name the energy risk that bears on the thesis \
and the level that would change the read. Fold this into the thesis as an inflation-and-growth \
qualifier — do not append it as a separate section.",
    },
    Skill {
        name: "Central Bank Interpretation",
        description: "Interprets central-bank communication, policy decisions, and market expectations.",
        body: "Central Bank Interpretation. Interpret central-bank policy, communication, and \
market expectations. Read the gathered policy signals — the fed-funds target range, the breakevens \
and the one-year expected-inflation gauge, the curve spreads, the financial-conditions indices, \
and the growth nowcast (GDPNow) — alongside the policy tone and rate-path expectations in the \
research packet. Judge the likely policy direction, where the market's rate expectations may be \
offside, and how the stance bears on equities, bonds, and liquidity. Name the policy read and the \
data or communication that would shift it. Fold this into the thesis as the rates-and-liquidity \
backdrop the rest of the report sits against — do not append it as a separate section.",
    },
    Skill {
        name: "Valuation Compression",
        description: "Analyzes how interest rates, yields, and macroeconomic conditions may affect valuation multiples.",
        body: "Valuation Compression. Analyze how rates, yields, and macro conditions bear on \
valuation multiples. Read the gathered valuation levels — the per-sector and per-industry P/E and \
the market risk premium — against the 10-year yield and the change view, focusing on long-duration \
growth assets and high-multiple sectors most sensitive to discount-rate moves. Judge whether \
earnings growth is sufficient to justify current multiples or whether rising yields threaten \
compression. Name the sectors most exposed and the yield move that would force a re-rating. Fold \
this into the thesis as the valuation risk to the equity call — do not append it as a separate \
section.",
    },
    Skill {
        name: "Consensus vs Contrarian Analysis",
        description: "Evaluates what the market currently expects versus what outcomes would genuinely surprise participants.",
        body: "Consensus vs Contrarian Analysis. Evaluate what the market expects versus what would \
genuinely surprise it. The economic calendar carries release names and dates but no consensus \
estimates, so source the consensus expectations from the research packet, then test them against \
the baseline, positioning, and the change view: identify overconsensus narratives, \
underappreciated risks, and asymmetric setups where positioning is vulnerable to an unexpected \
outcome. Look for where long-term expectations may be mispriced. Name the most crowded consensus \
view and the contrarian outcome with the largest asymmetry. Fold this into the thesis as the \
asymmetric-risk lens on the central call — do not append it as a separate section.",
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
        assert_eq!(CATALOG.len(), 16, "all 16 doc skills are authored");
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
