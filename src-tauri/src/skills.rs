//! The analyst skills library — a shared catalog of reusable analytical lenses
//! supplied to the main agent and the three Bull/Bear/Balanced analysts (`docs/analyst-skills.md`).
//!
//! Each skill carries a method [`Skill::body`] (the analytical lens — which baseline series
//! to read and how to judge them) and an explicit [`Skill::output`] verdict shape (the
//! structured conclusion the lens should yield). Each consumer supplies the **whole
//! library** ([`CATALOG`]) into its prompt in one pass via [`render_library`]; the model
//! applies the lenses the packet warrants and folds each verdict into its own output
//! (the main agent's unified thesis, an analyst's structured review). The bodies are
//! **consumer-neutral** — where each verdict lands is set by the per-consumer intro passed to
//! [`render_library`], not by the body — so the same lens serves both stages without pulling
//! an analyst toward final-synthesis behavior.
//!
//! Three design calls, all recorded against the doc:
//! - **Delivery:** the library is small (~150 tokens/skill), so all 16 skills ship in full
//!   every report. The doc's *progressive disclosure* (a phase-1 selection round-trip) was
//!   removed as net-negative at this size — it re-sent the whole packet to the model just to
//!   save the frontmatter catalog, paying a round-trip and a fail-soft code path to trim
//!   ~1.5k tokens. The model now self-selects which lenses apply, inline.
//! - **Output:** the per-skill [`Skill::output`] is a **forcing function** — it disciplines
//!   the model's per-lens verdict, which is then folded into the consumer's own output. It is
//!   *not* a machine-readable channel: nothing is parsed back from the report or persisted
//!   (the doc's per-skill output *schema* stays prose-level by design).
//! - **Consumers:** the main agent (during synthesis) and the three Bull/Bear/Balanced
//!   analysts (when forming their independent reviews) — the full original consumer set.

/// One analytical skill. `name` + `description` identify it — the `description` is taken
/// verbatim from `docs/analyst-skills.md`. `body` is the analytical method (the lens the
/// agent applies); `output` is the structured verdict the lens should yield — a forcing
/// function folded into the consumer's own output, never parsed back or persisted.
#[derive(Debug)]
pub struct Skill {
    pub name: &'static str,
    pub description: &'static str,
    pub body: &'static str,
    pub output: &'static str,
}

/// The authored skills library. Each `body` grounds its lens in baseline data the Step-3
/// scan already gathers and closes with the consumer-neutral role its verdict plays; each
/// `output` names the structured verdict the lens must produce. Extending the library is
/// purely additive — a follow-on appends entries here and they reach every consumer's prompt
/// automatically.
pub const CATALOG: &[Skill] = &[
    Skill {
        name: "Market Regime Analysis",
        description: "Determines the current market regime and the dominant forces driving market behavior.",
        body: "Market Regime Analysis. Determine the regime the market is in and the dominant \
force driving it. Read the baseline indices, internals, sector performance, and the change view \
together: judge whether conditions are risk-on or risk-off, liquidity-driven or earnings-driven, \
inflation-sensitive or growth-sensitive, and whether leadership is broadening or narrowing. Weigh \
the financial-conditions indices (NFCI / ANFCI / STLFSI4), the curve and credit spreads, and \
breadth (movers, sector dispersion, and whether small-caps — the Russell 2000 — lead or lag the large-cap indices) as regime evidence rather than isolated prints. Carry this read as the frame the rest of your analysis sits inside — \
not a standalone point.",
        output: "the regime (risk-on / risk-off, liquidity- vs earnings-driven, inflation- vs \
growth-sensitive, leadership broadening / narrowing); the one or two dominant forces that define \
it; and the single development that would mark a regime change.",
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
genuine tightening from spread noise. Carry the conclusion as a risk qualifier, not a \
standalone point.",
        output: "the spread level and direction (widening / compressing); whether credit-quality \
dispersion is rising; what it implies for refinancing and liquidity risk; and the spread move \
that would change the read.",
    },
    Skill {
        name: "Market Breadth Analysis",
        description: "Evaluates the health and participation level of the broader market beyond headline index performance.",
        body: "Market Breadth Analysis. Evaluate participation beneath the headline indices. Read \
the equity-breadth movers (gainers / losers / most-actives), the sector-performance dispersion, \
and the multi-horizon index performance — in particular whether small-caps (the Russell 2000) are \
confirming or lagging the large-cap indices — against the headline index moves: judge whether a \
rally or selloff is broad-based or narrow, whether leadership is concentrated, and how the NASDAQ \
growth read compares with the broader NYSE read. Treat a divergence between index level and \
breadth as a signal the index alone hides. Carry this as evidence on the durability of \
the index trend, not a standalone point.",
        output: "whether breadth confirms or contradicts the index trend; the leadership-\
concentration risk if breadth is narrow; and the breadth shift that would confirm or break the \
current move.",
    },
    Skill {
        name: "Narrative vs Reality",
        description: "Separates genuine market or economic changes from exaggerated media narratives and short-term emotional reactions.",
        body: "Narrative vs Reality. Separate genuine market and economic change from exaggerated \
media narratives and short-term emotional reactions. Take the dominant narratives surfaced in \
the research packet and test each against the baseline and its change view: do the indices, \
internals, breadth, credit spreads, financial-conditions indices, the earnings beats and misses, and the macro prints actually \
corroborate the story, or is the move headline-driven and unconfirmed by positioning and data? \
Carry the verdict as a confidence weight on each theme — not a standalone point.",
        output: "for each dominant narrative, whether the data corroborates or contradicts it; \
and the narrative most running ahead of the evidence.",
    },
    Skill {
        name: "Second-Order Effects",
        description: "Analyzes downstream consequences of major market, economic, geopolitical, or policy developments.",
        body: "Second-Order Effects. Map the downstream consequences of the major market, economic, \
geopolitical, or policy developments the change view or research packet surfaces. For each first-order move in the change \
view or the research packet, trace how it propagates into inflation (PCE, breakevens, expected \
inflation, oil and gas), yields and the curve (the 10-year yield, the 10y-3m and 10y-2y spreads), \
liquidity and financial conditions (NFCI / ANFCI / STLFSI4, credit spreads), sector performance, \
and consumer behavior (consumer sentiment, retail sales). Carry these as the \
forward consequences your analysis must price — not standalone points.",
        output: "the two or three second-order chains that matter most, each traced to its \
transmission (inflation / yields / liquidity / sectors / consumer); and the signal that would \
confirm each is playing out.",
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
persist. Carry the read as the inflation trajectory the rate and valuation views \
depend on — not a standalone point.",
        output: "which components are driving the print (energy / goods / services / shelter / \
wages); and whether the pressure is temporary or structural, broadening or narrowing.",
    },
    Skill {
        name: "Historical Analog",
        description: "Compares current market conditions to historical market environments and macroeconomic periods.",
        body: "Historical Analog. Compare current conditions to historical market and macro \
environments. Read the regime, valuations, credit, and the change view together, draw on vector \
memory and the recent reports for the thesis's own prior turns, and identify the closest analog — \
a dot-com-style valuation extreme, an inflationary or tightening cycle, a liquidity crisis, or a \
prior commodity or geopolitical shock. Be explicit about where the analog holds and where it \
breaks, so the comparison illuminates rather than forces a pattern. Carry this as historical context for your central read — not a \
standalone point.",
        output: "the single closest historical analog; where it holds; and the one difference \
that most limits the comparison.",
    },
    Skill {
        name: "Positioning & Sentiment",
        description: "Analyzes investor psychology, market positioning, and sentiment conditions.",
        body: "Positioning & Sentiment. Read investor psychology, positioning, and sentiment. Use \
the gathered tells — the volatility term structure (VIX against the 3-month VXV, and VXN for the \
Nasdaq), the equity-breadth movers and most-actives, sector dispersion, and consumer sentiment — \
alongside the positioning and flow color in the research packet to judge whether the market is \
fearful or greedy, complacent or capitulating, and where trades look crowded. Watch for euphoria, \
or for excessive pessimism that sets up an asymmetric reaction. Carry this as a contrarian check on your central read — not a \
standalone point.",
        output: "the current sentiment state (fear / greed, complacent / capitulating); and the \
crowded trade most exposed to a reversal.",
    },
    Skill {
        name: "Thesis Stress Test",
        description: "Challenges the current market thesis and searches for weak assumptions or contradictory evidence.",
        body: "Thesis Stress Test. Challenge the current thesis and hunt for its weak assumptions. \
Take the standing thesis from the recent reports and vector memory, then stress it against the \
change view, the data-gaps manifest, and any contradictory evidence in the baseline and research. \
Be adversarial — the goal is to find where the thesis is wrong, not to reaffirm it. Carry the findings as the stated risks and pivot triggers — not \
standalone points.",
        output: "the thesis's most fragile assumptions; the signals it is currently discounting; \
what could invalidate it; and the specific conditions that would force a reassessment or pivot.",
    },
    Skill {
        name: "Geopolitical Escalation",
        description: "Evaluates geopolitical developments and their potential market implications.",
        body: "Geopolitical Escalation. Evaluate geopolitical developments and their market \
implications. The baseline carries no geopolitical feed, so work from the conflicts, trade \
tensions, sanctions, shipping disruptions, and supply-chain risks surfaced in the research packet, \
and corroborate their market read against the gathered safe-haven and commodity moves — oil and \
natural gas, gold, the dollar, and credit spreads. Judge whether a development is a contained \
headline or a genuine escalation with commodity and supply-chain transmission. Carry this as a tail-risk qualifier — not a standalone \
point.",
        output: "the one or two geopolitical risks that bear on the thesis; whether each is a \
contained headline or a genuine escalation; and the market signal that would confirm escalation.",
    },
    Skill {
        name: "AI Infrastructure Chain",
        description: "Analyzes the AI infrastructure ecosystem and its broader market implications.",
        body: "AI Infrastructure Chain. Analyze the AI infrastructure ecosystem and its market \
implications. Anchor on what the baseline gathers — technology-sector performance, the \
semiconductor and related industries surfaced in the industry-level P/E and rotation snapshots, \
and the power-demand link through the energy series — and draw the chain detail (semiconductors, \
datacenters, HBM memory, networking, optics, cooling, capital expenditure) from the research \
packet. Judge whether the buildout is broadening or concentrating, whether valuations are \
supported by the capex trend, and where power and energy constraints bind. Carry this as the read on the market's dominant structural \
driver — not a standalone point.",
        output: "the segment leading the chain; whether the buildout is broadening or \
concentrating and whether valuations are supported by capex; and the signal that would mark a \
turn.",
    },
    Skill {
        name: "Time Horizon Separation",
        description: "Separates short-term market reactions from medium-term and long-term structural market trends.",
        body: "Time Horizon Separation. Separate short-term reactions from medium- and long-term \
structural trends. Use the cadence-honest change view to isolate this interval's moves, and read \
the multi-horizon index performance (weekly, month-to-date, year-to-date, and position in the \
52-week range) to place each move on its timescale; weigh them against the recent reports and \
vector memory to tell short-term noise from cyclical developments from structural long-term shifts. \
Resist letting a single volatile print pull the thesis; resist dismissing a slow structural \
change as noise. Carry this as the calibration between near-term commentary and \
the standing long-term view — not a standalone point.",
        output: "what is genuinely transient (short-term noise); and what is a durable \
cyclical or structural shift the thesis should absorb.",
    },
    Skill {
        name: "Energy Security Analysis",
        description: "Analyzes energy-market stability and the macroeconomic implications of energy disruptions.",
        body: "Energy Security Analysis. Assess energy-market stability and the macro implications \
of energy disruptions. Read the gathered energy levels — oil (WTI) and natural gas, with gold and \
the dollar as corroboration — and the change view, and draw OPEC activity, shipping chokepoints, \
and grid-stress detail from the research packet. Judge whether an energy move is a supply shock \
with inflation and growth consequences or ordinary volatility, and weigh the link between \
AI-infrastructure power demand and energy prices. Carry this as an inflation-and-growth qualifier — not a standalone \
point.",
        output: "whether the energy move is a supply shock or ordinary volatility; the energy risk \
that bears on the thesis; and the price level that would change the read.",
    },
    Skill {
        name: "Central Bank Interpretation",
        description: "Interprets central-bank communication, policy decisions, and market expectations.",
        body: "Central Bank Interpretation. Interpret central-bank policy, communication, and \
market expectations. Read the gathered policy signals — the fed-funds target range, the breakevens \
and the one-year expected-inflation gauge, the curve spreads, the financial-conditions indices, \
and the growth nowcast (GDPNow) — alongside the policy tone and rate-path expectations in the \
research packet. Judge the likely policy direction, where the market's rate expectations may be \
offside, and how the stance bears on equities, bonds, and liquidity. Carry this as the rates-and-liquidity backdrop the rest of your \
analysis sits against — not a standalone point.",
        output: "the likely policy direction; where market rate expectations may be offside; the \
implication for equities, bonds, and liquidity; and the data or communication that would shift \
the read.",
    },
    Skill {
        name: "Valuation Compression",
        description: "Analyzes how interest rates, yields, and macroeconomic conditions may affect valuation multiples.",
        body: "Valuation Compression. Analyze how rates, yields, and macro conditions bear on \
valuation multiples. Read the gathered valuation levels — the per-sector and per-industry P/E and \
the market risk premium — against the 10-year yield and the change view, focusing on long-duration \
growth assets and high-multiple sectors most sensitive to discount-rate moves. Judge whether \
earnings growth is sufficient to justify current multiples or whether rising yields threaten \
compression. Carry this as the valuation risk to the equity call — not a standalone \
point.",
        output: "whether earnings growth justifies current multiples; the sectors most exposed to \
discount-rate moves; and the yield move that would force a re-rating.",
    },
    Skill {
        name: "Consensus vs Contrarian Analysis",
        description: "Evaluates what the market currently expects versus what outcomes would genuinely surprise participants.",
        body: "Consensus vs Contrarian Analysis. Evaluate what the market expects versus what would \
genuinely surprise it. The economic calendar carries release names and dates but no consensus \
estimates — so source macro consensus from the research packet — while the earnings group already \
carries large-cap estimate-versus-actual EPS and revenue, a direct read on where results are \
beating or missing consensus. Test both against the baseline, positioning, and the change view: \
identify overconsensus narratives, underappreciated risks, and asymmetric setups where \
positioning is vulnerable to an unexpected outcome. Look for where long-term expectations may be \
mispriced. Carry this as the asymmetric-risk lens on your central read — not a \
standalone point.",
        output: "the most crowded consensus view; the underappreciated risk or contrarian outcome \
with the largest asymmetry; and where positioning is most vulnerable.",
    },
];

/// Render the whole [`CATALOG`] as a prompt block: the supplied `intro` followed by one
/// labeled sub-block per skill carrying its method `body` and the structured `output`
/// verdict it should yield (the `Verdict to produce —` forcing function). Returns an empty
/// string when the catalog is empty so a caller appends nothing.
///
/// The per-skill format and the verdict marker live here, in one place — both consumers
/// (the main agent's synthesis prompt and the three analysts' review prompts) supply their
/// own `intro` and share this body, so the marker convention can't drift between them. The
/// `intro` carries its own leading whitespace.
pub fn render_library(intro: &str) -> String {
    if CATALOG.is_empty() {
        return String::new();
    }
    let mut block = String::from(intro);
    for s in CATALOG {
        block.push_str(&format!(
            "\n\n--- {} ---\n{}\nVerdict to produce — {}",
            s.name, s.body, s.output
        ));
    }
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_entries_are_well_formed_and_uniquely_named() {
        assert!(
            !CATALOG.is_empty(),
            "the catalog must carry at least one skill"
        );
        assert_eq!(CATALOG.len(), 16, "all 16 doc skills are authored");
        for s in CATALOG {
            assert!(!s.name.trim().is_empty(), "a skill has a blank name");
            assert!(
                !s.description.trim().is_empty(),
                "{} has a blank description",
                s.name
            );
            assert!(!s.body.trim().is_empty(), "{} has a blank body", s.name);
            assert!(!s.output.trim().is_empty(), "{} has a blank output", s.name);
        }
        let names: Vec<&str> = CATALOG.iter().map(|s| s.name).collect();
        for (i, n) in names.iter().enumerate() {
            assert!(
                !names[i + 1..].contains(n),
                "duplicate skill name in the catalog: {n}"
            );
        }
    }

    #[test]
    fn render_library_carries_intro_every_skill_and_the_verdict_marker() {
        let block = render_library("\n\nINTRO-SENTINEL:");
        assert!(
            block.starts_with("\n\nINTRO-SENTINEL:"),
            "intro missing: {block}"
        );
        for s in CATALOG {
            assert!(block.contains(s.name), "missing skill name {}", s.name);
            assert!(block.contains(s.body), "missing body for {}", s.name);
            assert!(block.contains(s.output), "missing output for {}", s.name);
        }
        // The verdict marker is the forcing function — one source of truth for both consumers.
        assert!(block.contains("Verdict to produce —"), "{block}");
    }
}
