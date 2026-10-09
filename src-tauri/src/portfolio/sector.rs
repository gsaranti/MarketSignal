//! The sector identity a holding is read against — the profile sector label plus
//! its resolved SPDR sector-ETF benchmark (`docs/data-sources.md §Financial
//! Modeling Prep` — the sector-ETF mapping). It feeds the technology-event
//! pre-flag's benchmark leg and commodity routing, and rides the checkpoint
//! trail's run-level accumulators.

use serde::{Deserialize, Serialize};

/// The SPDR sector-ETF benchmark for an FMP profile sector label. Accepts both
/// FMP's label vocabulary and the near-identical GICS names; `None` for anything
/// else (the typed `sector-unscorable` path, never a guessed benchmark).
pub fn spdr_for_sector(label: &str) -> Option<&'static str> {
    let l = label.trim().to_ascii_lowercase();
    Some(match l.as_str() {
        "basic materials" | "materials" => "XLB",
        "communication services" => "XLC",
        "energy" => "XLE",
        "financial services" | "financials" => "XLF",
        "industrials" => "XLI",
        "technology" | "information technology" => "XLK",
        "consumer defensive" | "consumer staples" => "XLP",
        "real estate" => "XLRE",
        "utilities" => "XLU",
        "healthcare" | "health care" => "XLV",
        "consumer cyclical" | "consumer discretionary" => "XLY",
        _ => return None,
    })
}

/// A holding's **stamped sector identity** — the sector label in hand plus its
/// resolved SPDR benchmark symbol. A holding with no valid mapping carries the
/// typed `sector-unscorable` reason on its sector legs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectorIdentity {
    pub sector: Option<String>,
    pub benchmark: Option<String>,
    /// The typed `sector-unscorable` reason when no benchmark resolved.
    pub unscorable: Option<String>,
}

impl SectorIdentity {
    /// Resolve a profile sector label into the stamped identity.
    pub fn resolve(sector_label: Option<&str>) -> Self {
        match sector_label {
            Some(label) => match spdr_for_sector(label) {
                Some(etf) => Self {
                    sector: Some(label.to_string()),
                    benchmark: Some(etf.to_string()),
                    unscorable: None,
                },
                None => Self {
                    sector: Some(label.to_string()),
                    benchmark: None,
                    unscorable: Some(format!(
                        "sector-unscorable: no SPDR mapping for sector {label:?}"
                    )),
                },
            },
            None => Self::unscorable("no sector label resolved at the anchor run"),
        }
    }

    pub fn unscorable(reason: &str) -> Self {
        Self {
            sector: None,
            benchmark: None,
            unscorable: Some(format!("sector-unscorable: {reason}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spdr_map_covers_the_eleven_sectors_and_rejects_unknowns() {
        assert_eq!(spdr_for_sector("Technology"), Some("XLK"));
        assert_eq!(spdr_for_sector("consumer cyclical"), Some("XLY"));
        assert_eq!(spdr_for_sector("Health Care"), Some("XLV"));
        assert_eq!(spdr_for_sector("Healthcare"), Some("XLV"));
        assert_eq!(spdr_for_sector("Financial Services"), Some("XLF"));
        assert_eq!(spdr_for_sector("Space Mining"), None);
        let id = SectorIdentity::resolve(Some("Technology"));
        assert_eq!(id.benchmark.as_deref(), Some("XLK"));
        assert!(id.unscorable.is_none());
        let un = SectorIdentity::resolve(None);
        assert!(un.benchmark.is_none());
        assert!(un.unscorable.as_deref().unwrap().contains("sector-unscorable"));
    }
}
