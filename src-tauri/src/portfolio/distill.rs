//! Step-6d consolidation (`docs/portfolio-workflow.md` §Step 6d;
//! `docs/web-research.md §The research loop and context management`;
//! `docs/configuration.md §Research Context Management`).
//!
//! After a holding's research the orchestrator sizes the **analysis prompt**
//! — the holding header, FETCHED VALUES, on a continuity run PRIOR ANALYSIS,
//! then this run's write-ups — against the call's input budget. Within budget
//! the write-ups go in as written. Over it, the write-ups are **distilled**
//! first, in one of two shapes chosen deterministically from size and never
//! by the model: the merged write-ups in one call, or — where the merged
//! write-ups outgrow the widest issuable budget once rendered — each write-up
//! first and then the merge of those outputs. The prior analysis is never
//! distilled. Then the reasoner writes the holding's **analysis**, the only
//! research artifact the next run reads; the write-ups persist on the audit
//! as written, and the chosen shape with its call count persists beside them.
//! A holding whose loop produced no write-up spends no analysis call: the
//! prior analysis stands verbatim, or the one no-write-up sentence on a debut
//! (ruled 2026-10-08).
//!
//! The prompts are rendered here, pure; the calls go through the analyst
//! seam ([`crate::portfolio::pipeline::HoldingAnalyst`]), whose live
//! implementation sizes each rendered distillation prompt once more at issue.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::portfolio::pipeline::{HoldingAnalyst, NUM_CTX_INTERPRET};
use crate::portfolio::research::{HoldingBrief, HoldingResearch};
use crate::portfolio::AnalysisRecord;

// ---------------------------------------------------------------------------
// Constants (drafted, calibratable — `docs/configuration.md §Research Context
// Management`: generous, conservative defaults)
// ---------------------------------------------------------------------------

/// Fraction of a call's input budget above which the orchestrator takes the
/// next smaller shape (headroom left for the instruction scaffolding, the
/// output and, on the analysis call, the thinking trace).
pub const OVERFLOW_THRESHOLD: f64 = 0.6;

/// Rough chars-per-token for sizing a call's input budget off its `num_ctx`.
pub const CHARS_PER_TOKEN: f64 = 3.0;

/// The input budget for one call, derived from the resolved `num_ctx`.
pub fn input_budget_chars(num_ctx: u32) -> usize {
    (f64::from(num_ctx) * CHARS_PER_TOKEN * OVERFLOW_THRESHOLD) as usize
}

/// The one sentence that stands as a debut holding's analysis when its loop
/// produced no write-up — no call is spent over nothing.
pub const NO_WRITE_UP_SENTENCE: &str = "No research write-up this run.";

/// The disconfirming pass's heading under WRITE-UPS, the title it carries
/// everywhere the write-ups render together (the bridge of task 1 set it).
pub const CONTRARY_TITLE: &str = "Contrary evidence";

/// The disconfirming pass's key, for its per-write-up stage label.
pub const CONTRARY_KEY: &str = "disconfirming";

// ---------------------------------------------------------------------------
// The persisted record
// ---------------------------------------------------------------------------

/// The shape consolidation chose for a holding (`docs/portfolio-workflow.md`
/// §Step 6d), persisted on the research audit record so a distillation is
/// never silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DistillationShape {
    /// The analysis prompt fit its budget: the write-ups went in as written.
    None,
    /// The merged write-ups distilled in one call.
    Merged,
    /// Each write-up distilled first, then the merge of those outputs.
    PerWriteUp,
}

/// The shape with the distillation calls it spent — one on the merged shape,
/// the write-ups plus one on the per-write-up shape, none otherwise. The
/// expanded re-attempt a length stop earns is a retry of one of these calls,
/// not a call of the shape, and rides the tracker and the model ids instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistillationRecord {
    pub shape: DistillationShape,
    pub calls: u32,
}

impl DistillationRecord {
    /// No distillation: the analysis prompt fit, or no write-up existed.
    pub fn none() -> Self {
        Self { shape: DistillationShape::None, calls: 0 }
    }
}

// ---------------------------------------------------------------------------
// The inputs the seam's two calls read
// ---------------------------------------------------------------------------

/// One write-up as consolidation handles it: the topic's key (its stage
/// label), its title (its heading) and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteUp {
    pub key: String,
    pub title: String,
    pub text: String,
}

/// This run's write-ups in the order every message carries them — the topics
/// that wrote one, in agenda order, then the disconfirming pass's under
/// [`CONTRARY_TITLE`]. A topic with no write-up contributes nothing.
pub fn write_ups_of(research: &HoldingResearch) -> Vec<WriteUp> {
    let mut out: Vec<WriteUp> = research
        .topics
        .iter()
        .filter_map(|t| {
            t.write_up.as_ref().map(|text| WriteUp {
                key: t.topic_key.clone(),
                title: t.title.clone(),
                text: text.trim().to_string(),
            })
        })
        .collect();
    if let Some(text) = &research.disconfirming {
        out.push(WriteUp {
            key: CONTRARY_KEY.to_string(),
            title: CONTRARY_TITLE.to_string(),
            text: text.trim().to_string(),
        });
    }
    out
}

/// What one distillation call distills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistillSubject<'a> {
    /// The merged shape: every write-up, each under its topic's heading.
    Merged(&'a [WriteUp]),
    /// The per-write-up shape's first calls: one write-up under its heading.
    Single(&'a WriteUp),
    /// The per-write-up shape's last call: the merge of the per-write-up
    /// outputs, each under its topic's heading.
    MergedDistillates(&'a [WriteUp]),
}

/// What one distillation call reads: the holding header, its subject and the
/// stage label the call issues under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistillInput<'a> {
    pub header: &'a str,
    pub subject: DistillSubject<'a>,
    pub stage: String,
}

/// The WRITE-UPS block of the analysis message: the write-ups as written, or
/// their distillate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteUps<'a> {
    AsWritten(&'a [WriteUp]),
    Distilled(&'a str),
}

/// What the analysis call reads: the holding-constant brief (its header and
/// FETCHED VALUES — the bytes the gathering and synthesis messages share),
/// the rendered PRIOR ANALYSIS section on a continuity run (empty on a
/// debut), and the WRITE-UPS block.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisInput<'a> {
    pub symbol: &'a str,
    pub brief: &'a HoldingBrief,
    pub prior_analysis: &'a str,
    pub write_ups: WriteUps<'a>,
}

impl AnalysisInput<'_> {
    /// The stage label the analysis call issues under.
    pub fn stage(&self) -> String {
        format!("analysis {}", self.symbol)
    }
}

// ---------------------------------------------------------------------------
// The rendered prompts
// ---------------------------------------------------------------------------

/// One rendered two-message prompt: the role-line system message and the
/// two-part user message. The adapter seam sizes both messages at issue
/// (`pipeline::distill_route`), the same measure the budget check uses here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistillPrompt {
    pub system: String,
    pub user: String,
}

impl DistillPrompt {
    /// The rendered prompt's size as the issue guard measures it — both
    /// messages, in chars.
    pub fn chars(&self) -> usize {
        self.system.chars().count() + self.user.chars().count()
    }
}

/// The analysis prompt, the same two-message shape.
pub type AnalysisPrompt = DistillPrompt;

const PART_1: &str = "======== PART 1: INPUTS ========\n";
const PART_2: &str = "\n======== PART 2: TASK ========\n\n";
const PLAIN: &str = "as plain text — no code fence, no JSON, no heading before the first line";

/// The write-ups each under their heading — the block every message that
/// carries several write-ups renders, after its own gloss line.
fn write_ups_block(write_ups: &[WriteUp]) -> String {
    let mut out = String::new();
    for w in write_ups {
        out.push_str(&format!("\n{}\n{}\n", w.title, w.text.trim()));
    }
    out
}

/// The distillation prompt (`docs/portfolio-workflow.md` §Step 6d): the
/// holding header, then the write-ups to distill — the merged write-ups, or
/// one write-up on a per-write-up call, or the per-write-up outputs on the
/// merge call — each under its topic's heading; then the task: a shorter
/// document that keeps every dated figure with its source, every disagreement
/// between pages and every open question, within the distillation's length
/// band (`docs/portfolio-analysis.md` §Starting parameters). Consolidation,
/// not new reasoning.
pub fn distillation_prompt(input: &DistillInput<'_>) -> DistillPrompt {
    let system = "You are an investment analyst shortening research write-ups on one holding for \
a portfolio review. Part 1 of the message gives the inputs. Part 2 says what the document keeps \
and how to return it."
        .to_string();
    let mut user = String::from(PART_1);
    user.push_str(input.header);
    let (subject_word, plural) = match input.subject {
        DistillSubject::Merged(write_ups) => {
            user.push_str(
                "\nWRITE-UPS\nThis run's write-ups on the holding, each under its topic, the \
                 contrary-evidence pass last.\n",
            );
            user.push_str(&write_ups_block(write_ups));
            ("the write-ups under WRITE-UPS", true)
        }
        DistillSubject::Single(write_up) => {
            user.push_str("\nWRITE-UP\nOne of this run's write-ups on the holding, under its topic.\n");
            user.push_str(&write_ups_block(std::slice::from_ref(write_up)));
            ("the write-up under WRITE-UP", false)
        }
        DistillSubject::MergedDistillates(outputs) => {
            user.push_str(
                "\nWRITE-UPS\nThis run's write-ups on the holding, each already shortened, each \
                 under its topic, the contrary-evidence pass last.\n",
            );
            user.push_str(&write_ups_block(outputs));
            ("the write-ups under WRITE-UPS", true)
        }
    };
    let (states, notes, leaves, do_not, length_of) = if plural {
        ("state", "they note", "they leave", "do not", "the write-ups")
    } else {
        ("states", "it notes", "it leaves", "does not", "the write-up")
    };
    user.push_str(PART_2);
    user.push_str(&format!(
        "Write a shorter document {PLAIN}. It keeps every dated figure {subject_word} {states}, \
         with the date or period and the source each is given, every disagreement between pages \
         {notes}, and every question {leaves} open. It adds nothing {subject_word} {do_not} \
         say: this is consolidation, not new reasoning.\n\n\
         The document runs at most half the length of {length_of} and at most 1,200 words.\n"
    ));
    DistillPrompt { system, user }
}

/// The analysis prompt (`docs/portfolio-workflow.md` §Step 6d). Part 1, in
/// page order: the holding header, FETCHED VALUES (the brief's bytes), on a
/// continuity run PRIOR ANALYSIS, then WRITE-UPS — this run's write-ups each
/// under its topic's heading, the disconfirming pass's last, or their
/// distillate as one document. Part 2 asks for the analysis: one document
/// consolidating what this run's research established on the holding, each
/// dated figure with its source, where sources disagree, what stays
/// unanswered, and on a continuity run what the prior analysis said that this
/// run's research confirms, revises or leaves untouched, a topic with no
/// write-up this run keeping what the prior analysis says about it, within
/// the analysis's length band.
pub fn analysis_prompt(input: &AnalysisInput<'_>) -> AnalysisPrompt {
    let system = "You are an investment analyst consolidating one holding's research for a \
portfolio review. Part 1 of the message gives the inputs. Part 2 says what the analysis covers \
and how to return it."
        .to_string();
    let mut user = String::from(PART_1);
    user.push_str(&input.brief.header);
    user.push_str(&input.brief.fetched_values);
    user.push_str(input.prior_analysis);
    match input.write_ups {
        WriteUps::AsWritten(write_ups) => {
            user.push_str(
                "\nWRITE-UPS\nThis run's research on the holding, one write-up per topic, the \
                 contrary-evidence pass last.\n",
            );
            user.push_str(&write_ups_block(write_ups));
        }
        WriteUps::Distilled(distillate) => {
            user.push_str(
                "\nWRITE-UPS\nThis run's research on the holding, the topics' write-ups \
                 shortened into one document.\n",
            );
            user.push_str(&format!("\n{}\n", distillate.trim()));
        }
    }
    let source_clause = if input.brief.fetched_values.is_empty() {
        "the page address the write-up names"
    } else {
        "the page address the write-up names, or FETCHED VALUES where the figure comes from there"
    };
    user.push_str(PART_2);
    user.push_str(&format!(
        "Write the holding's analysis {PLAIN}. It consolidates what this run's research \
         established on the holding: what the write-ups under WRITE-UPS establish, where their \
         sources disagree, and what stays unanswered. Each figure is quoted with the date or \
         period its source gives for it, and its source is named: {source_clause}."
    ));
    if !input.prior_analysis.is_empty() {
        user.push_str(
            " It also states what the analysis under PRIOR ANALYSIS said that this run's research \
             confirms, revises or leaves untouched; a topic the write-ups do not cover this run \
             keeps what PRIOR ANALYSIS says about it.",
        );
    }
    user.push_str("\n\nThe analysis runs 900 to 1,800 words.\n");
    AnalysisPrompt { system, user }
}

// ---------------------------------------------------------------------------
// The shape choice and the orchestration
// ---------------------------------------------------------------------------

/// The two budgets the shape choice reads, in chars of rendered prompt: the
/// analysis call's, and the widest a distillation call can issue at (the
/// resident reasoner's — `docs/local-models.md §The local-model adapter seam`).
/// On the live roster both are the reasoner's interpretation context under
/// the overflow threshold; tests set them small to walk the shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsolidationBudgets {
    pub analysis: usize,
    pub widest: usize,
}

impl ConsolidationBudgets {
    /// The live budgets: the analysis call issues on the reasoner at
    /// [`NUM_CTX_INTERPRET`], and the widest distillation rung is the same
    /// context.
    pub fn live() -> Self {
        let chars = input_budget_chars(NUM_CTX_INTERPRET);
        Self { analysis: chars, widest: chars }
    }
}

/// The deterministic shape choice (`docs/web-research.md §The research loop
/// and context management`): the write-ups as written where the analysis
/// prompt fits; the merged write-ups distilled where it does not and the
/// merged distillation prompt fits the widest issuable budget; each write-up
/// first otherwise. The smaller shape is never taken for a prompt the
/// reasoner could serve.
pub fn choose_shape(
    analysis_chars: usize,
    merged_chars: usize,
    budgets: &ConsolidationBudgets,
) -> DistillationShape {
    if analysis_chars <= budgets.analysis {
        DistillationShape::None
    } else if merged_chars <= budgets.widest {
        DistillationShape::Merged
    } else {
        DistillationShape::PerWriteUp
    }
}

/// The analysis consolidation produced: this run's, written by the analysis
/// call — or the one no-write-up sentence on a debut — which the pipeline
/// stamps with this run's date and anchor bar; or the prior record carried
/// whole, its own date and anchor with it, where the loop wrote nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum Consolidated {
    Written(String),
    Carried(AnalysisRecord),
}

/// What consolidation returns: the holding's analysis and the record of how
/// it was reached.
#[derive(Debug, Clone, PartialEq)]
pub struct Consolidation {
    pub analysis: Consolidated,
    pub distillation: DistillationRecord,
}

/// The refusal an analysis prompt over its budget takes before issue — an
/// unclassified failure, never retried, since the outcome is deterministic
/// and the prior analysis is never distilled, so no smaller shape remains
/// (`docs/local-models.md §The local-model adapter seam`).
fn ensure_analysis_fits(stage: &str, chars: usize, budgets: &ConsolidationBudgets, when: &str) -> Result<()> {
    anyhow::ensure!(
        chars <= budgets.analysis,
        "{stage}: analysis prompt of {chars} chars exceeds the input budget ({} chars) {when} — \
         refused before issue; the prior analysis is never distilled and no smaller shape remains",
        budgets.analysis
    );
    Ok(())
}

/// Run Step 6d for one holding over the loop's output. `prior_analysis` is
/// the prior run's analysis record where one exists (the no-write-up
/// fallback); `prior_analysis_section` the rendered PRIOR ANALYSIS block the
/// analysis message carries (empty on a debut).
pub fn consolidate(
    model: &dyn HoldingAnalyst,
    symbol: &str,
    brief: &HoldingBrief,
    research: &HoldingResearch,
    prior_analysis: Option<&AnalysisRecord>,
    prior_analysis_section: &str,
    budgets: &ConsolidationBudgets,
) -> Result<Consolidation> {
    let write_ups = write_ups_of(research);
    if write_ups.is_empty() {
        // No write-up this run: no call over nothing — the prior analysis
        // stands, carried whole, else the one sentence (ruled 2026-10-08).
        return Ok(Consolidation {
            analysis: match prior_analysis {
                Some(record) => Consolidated::Carried(record.clone()),
                None => Consolidated::Written(NO_WRITE_UP_SENTENCE.to_string()),
            },
            distillation: DistillationRecord::none(),
        });
    }
    let as_written = AnalysisInput {
        symbol,
        brief,
        prior_analysis: prior_analysis_section,
        write_ups: WriteUps::AsWritten(&write_ups),
    };
    let stage = as_written.stage();
    let header = brief.header.as_str();
    let merged = DistillInput {
        header,
        subject: DistillSubject::Merged(&write_ups),
        stage: format!("distill {symbol}"),
    };
    let shape = choose_shape(
        analysis_prompt(&as_written).chars(),
        distillation_prompt(&merged).chars(),
        budgets,
    );
    if shape != DistillationShape::None {
        // Before any distillation call is spent: a prompt that would not fit
        // with no write-up at all — the header, FETCHED VALUES and the prior
        // analysis alone — cannot be shortened into budget.
        let floor = AnalysisInput { write_ups: WriteUps::Distilled(""), ..as_written.clone() };
        ensure_analysis_fits(&stage, analysis_prompt(&floor).chars(), budgets, "with no write-up")?;
    }
    let (distillate, calls) = match shape {
        DistillationShape::None => {
            let analysis = model
                .analyze(&as_written)
                .context("writing the holding's analysis")?;
            return Ok(Consolidation {
                analysis: Consolidated::Written(analysis),
                distillation: DistillationRecord::none(),
            });
        }
        DistillationShape::Merged => {
            let text = model
                .distill(&merged)
                .context("distilling the merged write-ups")?;
            (text, 1)
        }
        DistillationShape::PerWriteUp => {
            let mut outputs = Vec::with_capacity(write_ups.len());
            for w in &write_ups {
                let text = model
                    .distill(&DistillInput {
                        header,
                        subject: DistillSubject::Single(w),
                        stage: format!("distill {symbol} {}", w.key),
                    })
                    .with_context(|| format!("distilling the {} write-up", w.key))?;
                outputs.push(WriteUp { key: w.key.clone(), title: w.title.clone(), text });
            }
            // The merge of the per-write-up outputs is distilled once more,
            // unconditionally (`docs/portfolio-workflow.md` §Step 6d).
            let text = model
                .distill(&DistillInput {
                    header,
                    subject: DistillSubject::MergedDistillates(&outputs),
                    stage: format!("distill {symbol} merge"),
                })
                .context("distilling the merge of the per-write-up outputs")?;
            // The write-ups plus the merge: an agenda holds under ten topics,
            // so the count never nears the type's range.
            (text, write_ups.len() as u32 + 1)
        }
    };
    let over_distillate = AnalysisInput {
        symbol,
        brief,
        prior_analysis: prior_analysis_section,
        write_ups: WriteUps::Distilled(&distillate),
    };
    // The final prompt is sized once more with the distillate in: still over
    // the budget, it is refused rather than issued into silent truncation.
    ensure_analysis_fits(&stage, analysis_prompt(&over_distillate).chars(), budgets, "after distillation")?;
    let analysis = model
        .analyze(&over_distillate)
        .context("writing the holding's analysis over the distillate")?;
    Ok(Consolidation {
        analysis: Consolidated::Written(analysis),
        distillation: DistillationRecord { shape, calls },
    })
}

// ---------------------------------------------------------------------------
// The offline stub's renders
// ---------------------------------------------------------------------------

/// The offline stub's analysis: the write-ups as written under their titles,
/// the contrary pass last — or the distillate as handed — so a scripted
/// research run still yields the ANALYSIS the thesis document reads with no
/// model call.
pub fn stub_analysis(input: &AnalysisInput<'_>) -> String {
    match input.write_ups {
        WriteUps::AsWritten(write_ups) => write_ups_block(write_ups).trim().to_string(),
        WriteUps::Distilled(distillate) => distillate.trim().to_string(),
    }
}

/// The offline stub's distillate: each subject text cut to half its chars at
/// a character boundary, under its title where several render — a
/// deterministic stand-in for the model's shorter document.
pub fn stub_distillate(input: &DistillInput<'_>) -> String {
    fn half(text: &str) -> String {
        let text = text.trim();
        let keep = text.chars().count().div_ceil(2);
        text.chars().take(keep).collect::<String>().trim_end().to_string()
    }
    match input.subject {
        DistillSubject::Single(w) => half(&w.text),
        DistillSubject::Merged(ws) | DistillSubject::MergedDistillates(ws) => ws
            .iter()
            .map(|w| format!("{}\n{}", w.title, half(&w.text)))
            .collect::<Vec<_>>()
            .join("\n\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::research::TopicWriteUp;
    use std::cell::RefCell;

    fn topic(key: &str, title: &str, write_up: Option<&str>) -> TopicWriteUp {
        TopicWriteUp {
            topic_key: key.into(),
            title: title.into(),
            write_up: write_up.map(str::to_string),
            passes: usize::from(write_up.is_some()),
            skipped: None,
        }
    }

    fn research() -> HoldingResearch {
        HoldingResearch {
            topics: vec![
                topic("competitive-position", "Competitive / business position", Some("Share held.\n")),
                topic("results-revisions", "Recent results and estimate revisions", None),
                topic("catalysts-risks", "Catalysts and risks", Some("  A ruling is due in Q4.  ")),
            ],
            disconfirming: Some("Nothing contradicts the share read.".into()),
            ..Default::default()
        }
    }

    fn brief() -> HoldingBrief {
        HoldingBrief {
            header: "HOLDING\nWID (Widget Co).\nPrice: $10.00 per share.\nDate: 2026-08-22.\n".into(),
            fetched_values: "\nFETCHED VALUES\nQuote: 10.00 per share (the live print, undated).\n".into(),
            leads: vec![],
            prior_documents: String::new(),
        }
    }

    /// A scripted analyst that records every consolidation call it takes and
    /// answers each with a fixed text, so the shapes and their call counts
    /// pin offline.
    #[derive(Default)]
    struct Scripted {
        calls: RefCell<Vec<(String, String)>>,
    }

    impl HoldingAnalyst for Scripted {
        fn distill(&self, input: &DistillInput<'_>) -> Result<String> {
            let prompt = distillation_prompt(input);
            self.calls.borrow_mut().push((input.stage.clone(), prompt.user.clone()));
            Ok(format!("distillate of {}", input.stage))
        }
        fn analyze(&self, input: &AnalysisInput<'_>) -> Result<String> {
            let prompt = analysis_prompt(input);
            self.calls.borrow_mut().push((input.stage(), prompt.user.clone()));
            Ok("the analysis".into())
        }
        fn interpret(
            &self,
            input: &crate::portfolio::pipeline::ThesisInput,
        ) -> Result<crate::portfolio::PricedModelArm> {
            crate::portfolio::pipeline::StubAnalyst.interpret(input)
        }
        forward_stub_verdict_calls!();
    }

    #[test]
    fn the_write_ups_render_under_their_titles_with_the_contrary_pass_last() {
        let ws = write_ups_of(&research());
        assert_eq!(
            ws.iter().map(|w| (w.key.as_str(), w.title.as_str(), w.text.as_str())).collect::<Vec<_>>(),
            vec![
                ("competitive-position", "Competitive / business position", "Share held."),
                ("catalysts-risks", "Catalysts and risks", "A ruling is due in Q4."),
                (CONTRARY_KEY, CONTRARY_TITLE, "Nothing contradicts the share read."),
            ]
        );
        assert!(write_ups_of(&HoldingResearch::default()).is_empty());
        let stubbed = stub_analysis(&AnalysisInput {
            symbol: "WID",
            brief: &brief(),
            prior_analysis: "",
            write_ups: WriteUps::AsWritten(&ws),
        });
        assert_eq!(
            stubbed,
            "Competitive / business position\nShare held.\n\nCatalysts and risks\nA ruling is due \
             in Q4.\n\nContrary evidence\nNothing contradicts the share read."
        );
    }

    #[test]
    fn the_input_budget_is_the_chars_per_token_estimate_under_the_threshold() {
        assert_eq!(input_budget_chars(131_072), 235_929);
        assert_eq!(input_budget_chars(32_768), 58_982);
        let live = ConsolidationBudgets::live();
        assert_eq!(live.analysis, 235_929);
        assert_eq!(live.widest, 235_929);
    }

    #[test]
    fn the_shape_follows_the_two_budgets_and_never_shrinks_a_prompt_the_reasoner_could_serve() {
        let b = ConsolidationBudgets { analysis: 100, widest: 80 };
        assert_eq!(choose_shape(100, 500, &b), DistillationShape::None);
        assert_eq!(choose_shape(101, 80, &b), DistillationShape::Merged);
        assert_eq!(choose_shape(101, 81, &b), DistillationShape::PerWriteUp);
        // The analysis budget alone decides whether anything is distilled.
        assert_eq!(choose_shape(1, 1_000_000, &b), DistillationShape::None);
    }

    #[test]
    fn the_analysis_prompt_leads_with_the_brief_then_prior_analysis_then_the_write_ups() {
        let ws = write_ups_of(&research());
        let b = brief();
        let prior = "\nPRIOR ANALYSIS (written 2026-08-01)\nThe prior analysis.\n";
        let p = analysis_prompt(&AnalysisInput {
            symbol: "WID",
            brief: &b,
            prior_analysis: prior,
            write_ups: WriteUps::AsWritten(&ws),
        });
        assert!(p.system.starts_with("You are an investment analyst consolidating one holding's research"), "{}", p.system);
        let (part1, part2) = p.user.split_once(PART_2).expect("two parts");
        assert!(part1.starts_with(&format!("{PART_1}{}{}", b.header, b.fetched_values)), "{part1}");
        let at = |s: &str| part1.find(s).unwrap_or_else(|| panic!("{s}\n{part1}"));
        assert!(at("\nFETCHED VALUES\n") < at("\nPRIOR ANALYSIS (written 2026-08-01)\n"));
        assert!(at("\nPRIOR ANALYSIS (written 2026-08-01)\n") < at("\nWRITE-UPS\n"));
        assert!(part1.contains("\nWRITE-UPS\nThis run's research on the holding, one write-up per topic, the contrary-evidence pass last.\n\nCompetitive / business position\nShare held.\n\nCatalysts and risks\nA ruling is due in Q4.\n\nContrary evidence\nNothing contradicts the share read.\n"), "{part1}");
        assert!(part2.starts_with("Write the holding's analysis as plain text — no code fence, no JSON, no heading before the first line. It consolidates what this run's research established on the holding"), "{part2}");
        assert!(part2.contains("or FETCHED VALUES where the figure comes from there."), "{part2}");
        assert!(part2.contains(" It also states what the analysis under PRIOR ANALYSIS said that this run's research confirms, revises or leaves untouched; a topic the write-ups do not cover this run keeps what PRIOR ANALYSIS says about it."), "{part2}");
        assert!(part2.ends_with("\n\nThe analysis runs 900 to 1,800 words.\n"), "{part2}");
        assert_eq!(p.chars(), p.system.chars().count() + p.user.chars().count());

        // A debut: no PRIOR ANALYSIS section and no continuity clause; the
        // distillate renders as one document under its own gloss.
        let d = analysis_prompt(&AnalysisInput {
            symbol: "WID",
            brief: &b,
            prior_analysis: "",
            write_ups: WriteUps::Distilled("  The shortened document.  "),
        });
        assert!(!d.user.contains("PRIOR ANALYSIS"), "{}", d.user);
        assert!(d.user.contains("\nWRITE-UPS\nThis run's research on the holding, the topics' write-ups shortened into one document.\n\nThe shortened document.\n"), "{}", d.user);
        // No FETCHED VALUES on the brief: the source clause names the page alone.
        let bare = HoldingBrief { fetched_values: String::new(), ..b.clone() };
        let bare_prompt = analysis_prompt(&AnalysisInput {
            symbol: "WID",
            brief: &bare,
            prior_analysis: "",
            write_ups: WriteUps::AsWritten(&ws),
        });
        assert!(bare_prompt.user.contains("its source is named: the page address the write-up names.\n"), "{}", bare_prompt.user);
    }

    #[test]
    fn the_distillation_prompt_takes_the_three_subjects_under_one_task() {
        let ws = write_ups_of(&research());
        let header = "HOLDING\nWID (Widget Co).\n";
        let merged = distillation_prompt(&DistillInput {
            header,
            subject: DistillSubject::Merged(&ws),
            stage: "distill WID".into(),
        });
        assert!(merged.system.starts_with("You are an investment analyst shortening research write-ups"), "{}", merged.system);
        assert!(merged.user.starts_with(&format!("{PART_1}{header}\nWRITE-UPS\nThis run's write-ups on the holding, each under its topic, the contrary-evidence pass last.\n\nCompetitive / business position\nShare held.\n")), "{}", merged.user);
        assert!(merged.user.contains("\nContrary evidence\nNothing contradicts the share read.\n"), "{}", merged.user);
        let (_, task) = merged.user.split_once(PART_2).unwrap();
        assert_eq!(
            task,
            "Write a shorter document as plain text — no code fence, no JSON, no heading before the \
             first line. It keeps every dated figure the write-ups under WRITE-UPS state, with the \
             date or period and the source each is given, every disagreement between pages they \
             note, and every question they leave open. It adds nothing the write-ups under \
             WRITE-UPS do not say: this is consolidation, not new reasoning.\n\nThe document runs \
             at most half the length of the write-ups and at most 1,200 words.\n"
        );
        let single = distillation_prompt(&DistillInput {
            header,
            subject: DistillSubject::Single(&ws[1]),
            stage: "distill WID catalysts-risks".into(),
        });
        assert!(single.user.contains("\nWRITE-UP\nOne of this run's write-ups on the holding, under its topic.\n\nCatalysts and risks\nA ruling is due in Q4.\n"), "{}", single.user);
        assert!(!single.user.contains("Share held."), "{}", single.user);
        assert!(
            single.user.contains(
                "every dated figure the write-up under WRITE-UP states, with the date or period \
                 and the source each is given, every disagreement between pages it notes, and \
                 every question it leaves open. It adds nothing the write-up under WRITE-UP does \
                 not say: "
            ),
            "{}",
            single.user
        );
        assert!(single.user.ends_with("at most half the length of the write-up and at most 1,200 words.\n"), "{}", single.user);
        let outputs: Vec<WriteUp> = ws.iter().map(|w| WriteUp { key: w.key.clone(), title: w.title.clone(), text: format!("short {}", w.key) }).collect();
        let merge = distillation_prompt(&DistillInput {
            header,
            subject: DistillSubject::MergedDistillates(&outputs),
            stage: "distill WID merge".into(),
        });
        assert!(merge.user.contains("\nWRITE-UPS\nThis run's write-ups on the holding, each already shortened, each under its topic, the contrary-evidence pass last.\n\nCompetitive / business position\nshort competitive-position\n"), "{}", merge.user);
    }

    #[test]
    fn consolidation_takes_no_call_with_no_write_up_and_keeps_the_prior_analysis() {
        let model = Scripted::default();
        let b = brief();
        let none = HoldingResearch {
            topics: vec![topic("competitive-position", "Competitive / business position", None)],
            ..Default::default()
        };
        let out = consolidate(&model, "WID", &b, &none, None, "", &ConsolidationBudgets::live()).unwrap();
        assert_eq!(out.analysis, Consolidated::Written(NO_WRITE_UP_SENTENCE.into()));
        assert_eq!(out.distillation, DistillationRecord::none());
        // A prior record is carried whole — its own date and anchor with it.
        let prior = AnalysisRecord {
            text: "The prior analysis, verbatim.".into(),
            written: "2026-08-01".into(),
            anchor: Some(crate::portfolio::engine::DatedValue { date: "2026-07-31".into(), value: 9.5 }),
        };
        let out = consolidate(&model, "WID", &b, &none, Some(&prior), "\nPRIOR ANALYSIS\n...", &ConsolidationBudgets::live()).unwrap();
        assert_eq!(out.analysis, Consolidated::Carried(prior));
        assert_eq!(out.distillation, DistillationRecord::none());
        assert!(model.calls.borrow().is_empty(), "no call over nothing");
    }

    #[test]
    fn consolidation_refuses_an_analysis_prompt_still_over_budget_before_issue() {
        // The final prompt is sized once more with the distillate in; still
        // over the analysis budget it is refused before issue, unclassified,
        // after the distillation it spent. A prompt that would not fit with
        // no write-up at all is refused before any distillation call.
        let b = brief();
        let r = research();
        let ws = write_ups_of(&r);
        let chars_of = |write_ups: WriteUps<'_>| {
            analysis_prompt(&AnalysisInput { symbol: "WID", brief: &b, prior_analysis: "", write_ups }).chars()
        };
        let as_written = chars_of(WriteUps::AsWritten(&ws));
        let floor = chars_of(WriteUps::Distilled(""));
        let distilled = chars_of(WriteUps::Distilled("distillate of distill WID"));
        let merged_chars = distillation_prompt(&DistillInput {
            header: &b.header,
            subject: DistillSubject::Merged(&ws),
            stage: "distill WID".into(),
        })
        .chars();
        assert!(floor < distilled && distilled < as_written);

        // Over budget as written, the merged shape fits, the distilled prompt
        // still one char over: one distillation call, then the refusal.
        let model = Scripted::default();
        let err = consolidate(
            &model,
            "WID",
            &b,
            &r,
            None,
            "",
            &ConsolidationBudgets { analysis: distilled - 1, widest: merged_chars },
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("analysis WID: analysis prompt of"), "{msg}");
        assert!(msg.contains("after distillation — refused before issue"), "{msg}");
        assert_eq!(crate::local_model::retry_class(&err), None, "deterministic, never retried");
        assert_eq!(model.calls.take().iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), vec!["distill WID"]);

        // Even an empty WRITE-UPS block would not fit: refused before any
        // distillation call.
        let model = Scripted::default();
        let err = consolidate(
            &model,
            "WID",
            &b,
            &r,
            None,
            "",
            &ConsolidationBudgets { analysis: floor - 1, widest: merged_chars },
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("with no write-up — refused before issue"), "{err:#}");
        assert!(model.calls.borrow().is_empty(), "nothing spent");
    }

    #[test]
    fn consolidation_walks_the_three_shapes_with_their_call_counts() {
        let b = brief();
        let r = research();
        let ws = write_ups_of(&r);
        let as_written_chars = analysis_prompt(&AnalysisInput {
            symbol: "WID",
            brief: &b,
            prior_analysis: "",
            write_ups: WriteUps::AsWritten(&ws),
        })
        .chars();
        let merged_chars = distillation_prompt(&DistillInput {
            header: &b.header,
            subject: DistillSubject::Merged(&ws),
            stage: "distill WID".into(),
        })
        .chars();

        // Within budget: the write-ups as written, one analysis call, no shape.
        let model = Scripted::default();
        let out = consolidate(
            &model,
            "WID",
            &b,
            &r,
            None,
            "",
            &ConsolidationBudgets { analysis: as_written_chars, widest: merged_chars },
        )
        .unwrap();
        assert_eq!(out.analysis, Consolidated::Written("the analysis".into()));
        assert_eq!(out.distillation, DistillationRecord::none());
        let calls = model.calls.take();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "analysis WID");
        assert!(calls[0].1.contains("\nContrary evidence\nNothing contradicts the share read.\n"), "{}", calls[0].1);

        // One char over the analysis budget with the merged prompt within the
        // widest: the merged shape, one distillation call, then the analysis
        // over the distillate.
        let model = Scripted::default();
        let out = consolidate(
            &model,
            "WID",
            &b,
            &r,
            None,
            "",
            &ConsolidationBudgets { analysis: as_written_chars - 1, widest: merged_chars },
        )
        .unwrap();
        assert_eq!(out.distillation, DistillationRecord { shape: DistillationShape::Merged, calls: 1 });
        let calls = model.calls.take();
        assert_eq!(calls.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), vec!["distill WID", "analysis WID"]);
        assert!(calls[0].1.contains("\nWRITE-UPS\nThis run's write-ups on the holding, each under its topic, the contrary-evidence pass last.\n\nCompetitive / business position\nShare held.\n\nCatalysts and risks\n"), "{}", calls[0].1);
        assert!(calls[1].1.contains("\nWRITE-UPS\nThis run's research on the holding, the topics' write-ups shortened into one document.\n\ndistillate of distill WID\n"), "{}", calls[1].1);
        assert!(!calls[1].1.contains("Share held."), "the analysis reads the distillate, not the write-ups: {}", calls[1].1);

        // The merged prompt over the widest budget: each write-up first, the
        // contrary pass among them, then the merge of the outputs, then the
        // analysis — write-ups + 1 distillation calls.
        let model = Scripted::default();
        let prior = AnalysisRecord {
            text: "The prior analysis.".into(),
            written: "2026-08-01".into(),
            anchor: None,
        };
        let prior_section = "\nPRIOR ANALYSIS (written 2026-08-01)\nThe prior analysis.\n";
        // The budget is read against the prompt as it will render — PRIOR
        // ANALYSIS and the continuity clause included — one char under it.
        let as_written_with_prior = analysis_prompt(&AnalysisInput {
            symbol: "WID",
            brief: &b,
            prior_analysis: prior_section,
            write_ups: WriteUps::AsWritten(&ws),
        })
        .chars();
        let out = consolidate(
            &model,
            "WID",
            &b,
            &r,
            Some(&prior),
            prior_section,
            &ConsolidationBudgets { analysis: as_written_with_prior - 1, widest: merged_chars - 1 },
        )
        .unwrap();
        assert_eq!(out.distillation, DistillationRecord { shape: DistillationShape::PerWriteUp, calls: 4 });
        let calls = model.calls.take();
        assert_eq!(
            calls.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
            vec![
                "distill WID competitive-position",
                "distill WID catalysts-risks",
                "distill WID disconfirming",
                "distill WID merge",
                "analysis WID",
            ]
        );
        assert!(calls[0].1.contains("\nWRITE-UP\nOne of this run's write-ups on the holding, under its topic.\n\nCompetitive / business position\nShare held.\n"), "{}", calls[0].1);
        assert!(calls[3].1.contains("each already shortened, each under its topic, the contrary-evidence pass last.\n\nCompetitive / business position\ndistillate of distill WID competitive-position\n"), "{}", calls[3].1);
        assert!(calls[3].1.contains("\nContrary evidence\ndistillate of distill WID disconfirming\n"), "{}", calls[3].1);
        // The analysis message carries PRIOR ANALYSIS, never distilled, and
        // the merge's distillate.
        assert!(calls[4].1.contains("\nPRIOR ANALYSIS (written 2026-08-01)\nThe prior analysis.\n"), "{}", calls[4].1);
        assert!(calls[4].1.contains("\n\ndistillate of distill WID merge\n"), "{}", calls[4].1);
        assert!(calls[4].1.contains("what the analysis under PRIOR ANALYSIS said"), "{}", calls[4].1);
    }

    #[test]
    fn the_stub_distillate_halves_each_subject_and_keeps_the_titles() {
        let ws = write_ups_of(&research());
        let single = stub_distillate(&DistillInput {
            header: "",
            subject: DistillSubject::Single(&ws[0]),
            stage: String::new(),
        });
        assert_eq!(single, "Share");
        let merged = stub_distillate(&DistillInput {
            header: "",
            subject: DistillSubject::Merged(&ws),
            stage: String::new(),
        });
        assert_eq!(
            merged,
            "Competitive / business position\nShare\n\nCatalysts and risks\nA ruling is\n\nContrary \
             evidence\nNothing contradict"
        );
    }

    #[test]
    fn the_record_serializes_its_shape_in_kebab_case_and_requires_it_back() {
        let json = serde_json::to_value(DistillationRecord { shape: DistillationShape::PerWriteUp, calls: 4 }).unwrap();
        assert_eq!(json, serde_json::json!({"shape": "per-write-up", "calls": 4}));
        assert_eq!(serde_json::to_value(DistillationRecord::none()).unwrap(), serde_json::json!({"shape": "none", "calls": 0}));
        let back: DistillationRecord = serde_json::from_value(serde_json::json!({"shape": "merged", "calls": 1})).unwrap();
        assert_eq!(back, DistillationRecord { shape: DistillationShape::Merged, calls: 1 });
        assert!(serde_json::from_value::<DistillationRecord>(serde_json::json!({"calls": 1})).is_err());
    }
}
