//! Step-6d consolidation (`docs/portfolio-workflow.md` §Step 6d;
//! `docs/web-research.md §The research loop and context management`).
//!
//! The designed stage sizes the analysis prompt — the write-ups, FETCHED
//! VALUES and on a continuity run the prior analysis — against the call's
//! input budget, distills the write-ups in one of two shapes where it is over
//! (the merged write-ups, or each write-up first and then the merge of those
//! outputs), and has the reasoner write the holding's **analysis**, the only
//! research artifact the next run reads. Until that stage lands (the research
//! chain, task 2), this module is the bridge: the write-ups render under their
//! topic headings as the ANALYSIS block the thesis document reads, with no
//! model call and no budget check — no run happens between the two tasks by
//! ruling. The budget constants stay here for the gathering and synthesis
//! packet guards, which size against the same chars-per-token estimate.

use serde::{Deserialize, Serialize};

use crate::portfolio::research::HoldingResearch;

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

/// One rendered distillation prompt: the role-line system message and the
/// two-part user message. The adapter seam sizes both messages at issue
/// (`pipeline::distill_route`); parked until consolidation issues a call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistillPrompt {
    pub system: String,
    pub user: String,
}

impl DistillPrompt {
    /// The rendered prompt's size as the issue guard measures it — both
    /// messages, in chars.
    #[allow(dead_code)] // parked for consolidation (the research chain, task 2)
    pub fn chars(&self) -> usize {
        self.system.chars().count() + self.user.chars().count()
    }
}

/// The bridge ANALYSIS: this run's write-ups as written, each under its
/// topic's title, the disconfirming pass's last under its own — the order the
/// analysis message will carry its WRITE-UPS in. A topic with no write-up
/// this run renders nothing; a holding with none states so in one sentence.
pub fn bridge_analysis(research: &HoldingResearch) -> String {
    let mut out = String::new();
    for topic in &research.topics {
        let Some(write_up) = &topic.write_up else {
            continue;
        };
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("{}\n{}\n", topic.title, write_up.trim()));
    }
    if let Some(write_up) = &research.disconfirming {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("Contrary evidence\n{}\n", write_up.trim()));
    }
    if out.is_empty() {
        out.push_str("No research write-up this run.");
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::research::TopicWriteUp;

    fn topic(key: &str, title: &str, write_up: Option<&str>) -> TopicWriteUp {
        TopicWriteUp {
            topic_key: key.into(),
            title: title.into(),
            write_up: write_up.map(str::to_string),
            passes: usize::from(write_up.is_some()),
            skipped: None,
        }
    }

    #[test]
    fn the_bridge_renders_write_ups_under_their_titles_with_the_contrary_pass_last() {
        let research = HoldingResearch {
            topics: vec![
                topic("competitive-position", "Competitive / business position", Some("Share held.\n")),
                topic("results-revisions", "Recent results and estimate revisions", None),
                topic("catalysts-risks", "Catalysts and risks", Some("  A ruling is due in Q4.  ")),
            ],
            disconfirming: Some("Nothing contradicts the share read.".into()),
            ..Default::default()
        };
        assert_eq!(
            bridge_analysis(&research),
            "Competitive / business position\nShare held.\n\nCatalysts and risks\nA ruling is due \
             in Q4.\n\nContrary evidence\nNothing contradicts the share read."
        );
    }

    #[test]
    fn the_bridge_states_an_empty_run_in_one_sentence() {
        let research = HoldingResearch {
            topics: vec![topic("competitive-position", "Competitive / business position", None)],
            ..Default::default()
        };
        assert_eq!(bridge_analysis(&research), "No research write-up this run.");
        assert_eq!(bridge_analysis(&HoldingResearch::default()), "No research write-up this run.");
    }

    #[test]
    fn the_input_budget_is_the_chars_per_token_estimate_under_the_threshold() {
        assert_eq!(input_budget_chars(131_072), 235_929);
        assert_eq!(input_budget_chars(32_768), 58_982);
    }
}
