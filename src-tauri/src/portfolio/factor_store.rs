//! The factor-distribution store: one current observation per issuer per
//! factor, bucketed by profile sector × market-cap band, contributed by both
//! jobs' engine passes (`docs/storage.md §Local Analysis Suite Storage`;
//! `docs/trade-opportunities.md` §Starting parameters). Diagnostic-only, never
//! a score input: a selected sample of what the app happened to analyze is no
//! market percentile at any weight.

use anyhow::{Context, Result};
use chrono::{Months, NaiveDate};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::factors::{Factor, FactorComposite};

// ---------------------------------------------------------------------------
// Calibration surface — drafted, calibratable (`docs/trade-opportunities.md`
// §Starting parameters).
// ---------------------------------------------------------------------------

/// An observation dated more than this many calendar months before the session
/// ages out, so the distribution stays regime-current.
pub const MAX_AGE_MONTHS: u32 = 24;

/// The fewest other issuers a bucket must hold for a factor before the read
/// places a value among them.
pub const ISSUER_FLOOR: usize = 20;

/// The market-cap band floors in dollars, each lower bound inclusive; below the
/// small-cap floor a name is a micro-cap.
pub const MEGA_CAP_FLOOR: f64 = 200e9;
pub const LARGE_CAP_FLOOR: f64 = 10e9;
pub const MID_CAP_FLOOR: f64 = 2e9;
pub const SMALL_CAP_FLOOR: f64 = 300e6;

// ---------------------------------------------------------------------------
// Market-cap bands.
// ---------------------------------------------------------------------------

/// A name's market-cap band — the store's bucket and discovery's stratum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapBand {
    Mega,
    Large,
    Mid,
    Small,
    Micro,
}

impl CapBand {
    /// The band's stored key.
    pub fn key(self) -> &'static str {
        match self {
            CapBand::Mega => "mega",
            CapBand::Large => "large",
            CapBand::Mid => "mid",
            CapBand::Small => "small",
            CapBand::Micro => "micro",
        }
    }

    /// The band a stored key names.
    pub fn from_key(key: &str) -> Option<Self> {
        [
            CapBand::Mega,
            CapBand::Large,
            CapBand::Mid,
            CapBand::Small,
            CapBand::Micro,
        ]
        .into_iter()
        .find(|band| band.key() == key)
    }
}

/// The band of a market capitalization; `None` for a non-finite or
/// non-positive one.
pub fn cap_band(market_cap: f64) -> Option<CapBand> {
    if !market_cap.is_finite() || market_cap <= 0.0 {
        return None;
    }
    Some(if market_cap >= MEGA_CAP_FLOOR {
        CapBand::Mega
    } else if market_cap >= LARGE_CAP_FLOOR {
        CapBand::Large
    } else if market_cap >= MID_CAP_FLOOR {
        CapBand::Mid
    } else if market_cap >= SMALL_CAP_FLOOR {
        CapBand::Small
    } else {
        CapBand::Micro
    })
}

// ---------------------------------------------------------------------------
// Schema.
// ---------------------------------------------------------------------------

/// Create the store's table if absent. Idempotent.
///
/// The `(issuer, factor)` primary key and the value / floored check are
/// mirrored by the import pre-checks in `portability::import_archive`. The
/// sector compares case-insensitively, so a profile label served in another
/// case reads the same bucket.
pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS factor_observations (
            issuer      TEXT NOT NULL,
            factor      TEXT NOT NULL,
            sector      TEXT NOT NULL COLLATE NOCASE,
            cap_band    TEXT NOT NULL,
            value       REAL,
            floored     INTEGER NOT NULL,
            observed_on TEXT NOT NULL,
            PRIMARY KEY (issuer, factor),
            CHECK ((value IS NULL) = (floored = 1))
        )",
        [],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Readings.
// ---------------------------------------------------------------------------

/// A factor's resolved reading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FactorReading {
    /// The factor's raw value.
    Level(f64),
    /// No finite level, but the worst read the factor holds — debt over a
    /// non-positive equity. It ranks above every finite value.
    Floored,
}

impl FactorReading {
    /// `(value, floored)` as stored.
    fn columns(self) -> (Option<f64>, bool) {
        match self {
            FactorReading::Level(v) => (Some(v), false),
            FactorReading::Floored => (None, true),
        }
    }
}

/// Every factor of a composite that resolved, with its reading; an imputed
/// factor has none.
pub fn resolved_readings(composite: &FactorComposite) -> Vec<(Factor, FactorReading)> {
    composite
        .factors
        .iter()
        .filter_map(|score| {
            if score.floored {
                Some((score.factor, FactorReading::Floored))
            } else {
                score
                    .raw
                    .filter(|v| v.is_finite())
                    .map(|v| (score.factor, FactorReading::Level(v)))
            }
        })
        .collect()
}

/// A factor's stored key — its serialized name. A factor that serializes to
/// anything but a name is an error, never a panic: the write that asks is
/// fail-soft.
pub fn factor_key(factor: Factor) -> Result<String> {
    match serde_json::to_value(factor)? {
        serde_json::Value::String(key) => Ok(key),
        other => anyhow::bail!("factor {factor:?} serialized to {other}, not a name"),
    }
}

/// The factor a stored key names.
pub fn factor_from_key(key: &str) -> Option<Factor> {
    serde_json::from_value(serde_json::Value::String(key.to_string())).ok()
}

/// The oldest observation date still current at `session`.
fn oldest_current(session: NaiveDate) -> Result<String> {
    session
        .checked_sub_months(Months::new(MAX_AGE_MONTHS))
        .map(|d| d.format("%Y-%m-%d").to_string())
        .context("the age-out cutoff precedes the calendar")
}

// ---------------------------------------------------------------------------
// The write.
// ---------------------------------------------------------------------------

/// What a contribution did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contribution {
    /// The issuer's observations were replaced by this many readings.
    Written(usize),
    /// No sector label — the name cannot be bucketed, and nothing was written.
    NoSector,
    /// No positive, finite market cap — likewise.
    NoMarketCap,
}

/// Record an engine pass's composite as the issuer's current observations, in
/// one transaction: every prior row of the issuer is deleted, each resolved
/// factor is written under the session date, and every observation that has
/// aged out by the session is pruned. A name with no sector or no market cap
/// writes nothing and leaves its prior rows as they were.
pub fn contribute(
    conn: &Connection,
    symbol: &str,
    sector: Option<&str>,
    market_cap: Option<f64>,
    composite: &FactorComposite,
    session: NaiveDate,
) -> Result<Contribution> {
    let Some(sector) = sector.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Contribution::NoSector);
    };
    let Some(band) = market_cap.and_then(cap_band) else {
        return Ok(Contribution::NoMarketCap);
    };
    let issuer = symbol.trim().to_ascii_uppercase();
    let observed_on = session.format("%Y-%m-%d").to_string();
    let readings = resolved_readings(composite);

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM factor_observations WHERE issuer = ?1",
        params![issuer],
    )?;
    for (factor, reading) in &readings {
        let (value, floored) = reading.columns();
        tx.execute(
            "INSERT INTO factor_observations
                 (issuer, factor, sector, cap_band, value, floored, observed_on)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                issuer,
                factor_key(*factor)?,
                sector,
                band.key(),
                value,
                floored as i64,
                observed_on
            ],
        )?;
    }
    tx.execute(
        "DELETE FROM factor_observations WHERE observed_on < ?1",
        params![oldest_current(session)?],
    )?;
    tx.commit()?;
    Ok(Contribution::Written(readings.len()))
}

// ---------------------------------------------------------------------------
// The read.
// ---------------------------------------------------------------------------

/// Where a reading sits among the bucket's other issuers on one factor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BucketPosition {
    /// The reading's mid-rank percentile among their readings, 0–100, low to
    /// high — a position, with no polarity applied.
    pub percentile: f64,
    /// The other issuers the percentile reads (≥ [`ISSUER_FLOOR`]).
    pub issuers: usize,
}

/// Place a reading among the current observations of the bucket's other
/// issuers on the same factor: the asking issuer's own row and every row aged
/// out by the session are left out. `None` while fewer than [`ISSUER_FLOOR`]
/// issuers remain, and for a non-finite reading, which has no position.
pub fn bucket_position(
    conn: &Connection,
    symbol: &str,
    sector: &str,
    band: CapBand,
    factor: Factor,
    reading: FactorReading,
    session: NaiveDate,
) -> Result<Option<BucketPosition>> {
    if matches!(reading, FactorReading::Level(v) if !v.is_finite()) {
        return Ok(None);
    }
    let issuer = symbol.trim().to_ascii_uppercase();
    let mut stmt = conn.prepare(
        "SELECT value, floored FROM factor_observations
         WHERE sector = ?1 AND cap_band = ?2 AND factor = ?3
           AND issuer <> ?4 AND observed_on >= ?5",
    )?;
    let others = stmt
        .query_map(
            params![
                sector.trim(),
                band.key(),
                factor_key(factor)?,
                issuer,
                oldest_current(session)?
            ],
            |r| {
                let value: Option<f64> = r.get(0)?;
                let floored: bool = r.get(1)?;
                Ok(match value {
                    Some(v) if !floored => FactorReading::Level(v),
                    _ => FactorReading::Floored,
                })
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if others.len() < ISSUER_FLOOR {
        return Ok(None);
    }
    let (below, equal) = others.iter().fold((0usize, 0usize), |(below, equal), other| {
        match (reading, *other) {
            (FactorReading::Level(v), FactorReading::Level(o)) if o < v => (below + 1, equal),
            (FactorReading::Level(v), FactorReading::Level(o)) if o == v => (below, equal + 1),
            (FactorReading::Floored, FactorReading::Level(_)) => (below + 1, equal),
            (FactorReading::Floored, FactorReading::Floored) => (below, equal + 1),
            _ => (below, equal),
        }
    });
    Ok(Some(BucketPosition {
        percentile: 100.0 * (below as f64 + 0.5 * equal as f64) / others.len() as f64,
        issuers: others.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::factors::{Band, BandBasis, FactorScore, MetricClass};

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn score(factor: Factor, raw: Option<f64>, floored: bool) -> FactorScore {
        FactorScore {
            factor,
            raw,
            floored,
            band: Band {
                anchor: 0.05,
                half_width: 0.05,
                basis: BandBasis::Sector,
            },
            winsorized: false,
            band_score: raw.map(|_| 50.0),
            own_history: None,
            score: 50.0,
            imputed: raw.is_none() && !floored,
        }
    }

    fn composite(factors: Vec<FactorScore>) -> FactorComposite {
        FactorComposite {
            archetype: None,
            class: MetricClass::General,
            sector_anchor: "Technology".into(),
            factors,
            axes: Vec::new(),
            composite: 50.0,
            thin_own_history: false,
        }
    }

    fn earnings_yield(v: f64) -> FactorComposite {
        composite(vec![score(Factor::EarningsYield, Some(v), false)])
    }

    /// `(issuer, factor, sector, cap band, value, floored, observed on)`, ordered.
    #[allow(clippy::type_complexity)]
    fn rows(
        conn: &Connection,
    ) -> Vec<(String, String, String, String, Option<f64>, bool, String)> {
        conn.prepare(
            "SELECT issuer, factor, sector, cap_band, value, floored, observed_on
             FROM factor_observations ORDER BY issuer, factor",
        )
        .unwrap()
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    /// Contribute a pass whose sector and market cap are both present.
    fn put(
        conn: &Connection,
        symbol: &str,
        sector: &str,
        cap: f64,
        pass: &FactorComposite,
        on: &str,
    ) {
        contribute(conn, symbol, Some(sector), Some(cap), pass, day(on)).unwrap();
    }

    /// Seed `n` issuers' earnings yields `0.01, 0.02, …` in the Technology
    /// mid-cap bucket on `on`.
    fn seed_bucket(conn: &Connection, n: usize, on: &str) {
        for i in 1..=n {
            contribute(
                conn,
                &format!("T{i:03}"),
                Some("Technology"),
                Some(5e9),
                &earnings_yield(i as f64 / 100.0),
                day(on),
            )
            .unwrap();
        }
    }

    #[test]
    fn the_cap_bands_read_their_lower_bounds_inclusively() {
        assert_eq!(cap_band(200e9), Some(CapBand::Mega));
        assert_eq!(cap_band(199.9e9), Some(CapBand::Large));
        assert_eq!(cap_band(10e9), Some(CapBand::Large));
        assert_eq!(cap_band(9.99e9), Some(CapBand::Mid));
        assert_eq!(cap_band(2e9), Some(CapBand::Mid));
        assert_eq!(cap_band(1.99e9), Some(CapBand::Small));
        assert_eq!(cap_band(300e6), Some(CapBand::Small));
        assert_eq!(cap_band(299e6), Some(CapBand::Micro));
        assert_eq!(cap_band(1.0), Some(CapBand::Micro));
        assert_eq!(cap_band(0.0), None);
        assert_eq!(cap_band(-5e9), None);
        assert_eq!(cap_band(f64::NAN), None);
        assert_eq!(cap_band(f64::INFINITY), None);
        for band in [
            CapBand::Mega,
            CapBand::Large,
            CapBand::Mid,
            CapBand::Small,
            CapBand::Micro,
        ] {
            assert_eq!(CapBand::from_key(band.key()), Some(band));
            assert_eq!(
                serde_json::to_value(band).unwrap(),
                serde_json::Value::String(band.key().into())
            );
        }
        assert_eq!(CapBand::from_key("nano"), None);
    }

    #[test]
    fn the_schema_init_is_idempotent() {
        let conn = conn();
        init_schema(&conn).unwrap();
        contribute(
            &conn,
            "AAPL",
            Some("Technology"),
            Some(3e12),
            &earnings_yield(0.03),
            day("2026-10-14"),
        )
        .unwrap();
        init_schema(&conn).unwrap();
        assert_eq!(rows(&conn).len(), 1);
    }

    #[test]
    fn a_contribution_stores_each_resolved_reading_and_skips_the_imputed() {
        let conn = conn();
        let pass = composite(vec![
            score(Factor::EarningsYield, Some(0.04), false),
            score(Factor::DebtToEquity, None, true),
            score(Factor::Revision, None, false),
        ]);
        let written = contribute(
            &conn,
            " aapl ",
            Some(" Technology "),
            Some(3e12),
            &pass,
            day("2026-10-14"),
        )
        .unwrap();
        assert_eq!(written, Contribution::Written(2));
        assert_eq!(
            rows(&conn),
            vec![
                (
                    "AAPL".into(),
                    "debt-to-equity".into(),
                    "Technology".into(),
                    "mega".into(),
                    None,
                    true,
                    "2026-10-14".into()
                ),
                (
                    "AAPL".into(),
                    "earnings-yield".into(),
                    "Technology".into(),
                    "mega".into(),
                    Some(0.04),
                    false,
                    "2026-10-14".into()
                ),
            ]
        );
        assert_eq!(factor_from_key("no-such-factor"), None);
    }

    #[test]
    fn every_factor_keys_by_its_serialized_name_and_back() {
        use crate::portfolio::factors::{factor_set, Archetype};
        let classes = [
            MetricClass::General,
            MetricClass::Bank,
            MetricClass::OtherFinancial,
            MetricClass::EquityReit,
        ];
        for class in classes {
            for archetype in [None, Some(Archetype::CommodityCyclical)] {
                for factor in factor_set(archetype, class) {
                    let key = factor_key(factor).unwrap();
                    assert_eq!(serde_json::to_value(factor).unwrap(), key.as_str());
                    assert_eq!(factor_from_key(&key), Some(factor));
                }
            }
        }
        assert_eq!(factor_key(Factor::DebtToEquity).unwrap(), "debt-to-equity");
    }

    #[test]
    fn a_reanalysis_replaces_the_issuers_observations_never_appends() {
        let conn = conn();
        let first = composite(vec![
            score(Factor::EarningsYield, Some(0.04), false),
            score(Factor::Momentum, Some(0.12), false),
        ]);
        contribute(&conn, "AAPL", Some("Technology"), Some(3e12), &first, day("2026-09-01"))
            .unwrap();
        // The next pass: momentum no longer resolves, and the name fell a band.
        let second = composite(vec![
            score(Factor::EarningsYield, Some(0.05), false),
            score(Factor::Momentum, None, false),
        ]);
        contribute(&conn, "AAPL", Some("Technology"), Some(150e9), &second, day("2026-10-14"))
            .unwrap();
        assert_eq!(
            rows(&conn),
            vec![(
                "AAPL".into(),
                "earnings-yield".into(),
                "Technology".into(),
                "large".into(),
                Some(0.05),
                false,
                "2026-10-14".into()
            )]
        );
    }

    #[test]
    fn a_name_with_no_sector_or_market_cap_writes_nothing() {
        let conn = conn();
        put(&conn, "AAPL", "Technology", 3e12, &earnings_yield(0.03), "2026-09-01");
        let before = rows(&conn);
        for (sector, cap, outcome) in [
            (None, Some(3e12), Contribution::NoSector),
            (Some("  "), Some(3e12), Contribution::NoSector),
            (Some("Technology"), None, Contribution::NoMarketCap),
            (Some("Technology"), Some(0.0), Contribution::NoMarketCap),
        ] {
            assert_eq!(
                contribute(&conn, "AAPL", sector, cap, &earnings_yield(0.09), day("2026-10-14"))
                    .unwrap(),
                outcome
            );
        }
        assert_eq!(rows(&conn), before, "the prior observation stands");
    }

    #[test]
    fn a_write_prunes_every_observation_aged_out_by_its_session() {
        let conn = conn();
        put(&conn, "OLD", "Energy", 5e9, &earnings_yield(0.08), "2024-10-13");
        put(&conn, "EDGE", "Energy", 5e9, &earnings_yield(0.08), "2024-10-14");
        put(&conn, "NEW", "Utilities", 5e9, &earnings_yield(0.05), "2026-10-14");
        let issuers: Vec<String> = rows(&conn).into_iter().map(|r| r.0).collect();
        assert_eq!(issuers, vec!["EDGE".to_string(), "NEW".to_string()]);
    }

    #[test]
    fn the_read_waits_for_twenty_other_issuers() {
        let conn = conn();
        seed_bucket(&conn, 19, "2026-10-01");
        let read = |conn: &Connection, symbol: &str| {
            bucket_position(
                conn,
                symbol,
                "Technology",
                CapBand::Mid,
                Factor::EarningsYield,
                FactorReading::Level(0.105),
                day("2026-10-14"),
            )
            .unwrap()
        };
        assert_eq!(read(&conn, "ASK"), None, "19 issuers stay under the floor");
        seed_bucket(&conn, 20, "2026-10-01");
        assert_eq!(
            read(&conn, "ASK"),
            Some(BucketPosition { percentile: 50.0, issuers: 20 })
        );
        // The asking issuer's own row never counts toward the floor.
        assert_eq!(read(&conn, "T020"), None);
    }

    #[test]
    fn the_read_skips_aged_out_rows_and_other_buckets() {
        let conn = conn();
        seed_bucket(&conn, 20, "2026-10-01");
        let read = |sector: &str, band: CapBand, session: &str| {
            bucket_position(
                &conn,
                "ASK",
                sector,
                band,
                Factor::EarningsYield,
                FactorReading::Level(0.105),
                day(session),
            )
            .unwrap()
        };
        assert!(
            read("technology", CapBand::Mid, "2026-10-14").is_some(),
            "the sector matches in any case"
        );
        assert_eq!(read("Energy", CapBand::Mid, "2026-10-14"), None);
        assert_eq!(read("Technology", CapBand::Small, "2026-10-14"), None);
        // Two years and a day on, the seeded rows have aged out of the read
        // though no write has pruned them.
        assert!(read("Technology", CapBand::Mid, "2028-10-01").is_some());
        assert_eq!(read("Technology", CapBand::Mid, "2028-10-02"), None);
    }

    #[test]
    fn the_floor_counts_the_issuers_holding_that_factor() {
        let conn = conn();
        // Twenty issuers in the bucket, only ten of which resolved momentum.
        for i in 1..=20 {
            let mut factors = vec![score(Factor::EarningsYield, Some(0.05), false)];
            if i <= 10 {
                factors.push(score(Factor::Momentum, Some(0.1), false));
            }
            let symbol = format!("T{i:03}");
            put(&conn, &symbol, "Technology", 5e9, &composite(factors), "2026-10-01");
        }
        let read = |factor: Factor| {
            let level = FactorReading::Level(0.05);
            let session = day("2026-10-14");
            bucket_position(&conn, "ASK", "Technology", CapBand::Mid, factor, level, session)
                .unwrap()
        };
        assert!(read(Factor::EarningsYield).is_some());
        assert_eq!(read(Factor::Momentum), None);
    }

    #[test]
    fn the_read_is_a_mid_rank_value_percentile_with_floored_readings_on_top() {
        let conn = conn();
        // Eighteen finite yields 0.01–0.18 and two floored debt / equity reads
        // share the bucket — the floored pair on the debt factor alone.
        for i in 1..=20 {
            let reading = if i > 18 {
                score(Factor::DebtToEquity, None, true)
            } else {
                score(Factor::DebtToEquity, Some(i as f64 / 100.0), false)
            };
            let symbol = format!("T{i:03}");
            put(&conn, &symbol, "Technology", 5e9, &composite(vec![reading]), "2026-10-01");
        }
        let read = |reading: FactorReading| {
            let factor = Factor::DebtToEquity;
            let session = day("2026-10-14");
            bucket_position(&conn, "ASK", "Technology", CapBand::Mid, factor, reading, session)
                .unwrap()
                .unwrap()
                .percentile
        };
        // Below every reading, above the finite ones, tied with the 0.05 read,
        // and floored itself — tied with the two floored ones above every
        // finite value. No polarity applies: a higher leverage reads higher.
        assert_eq!(read(FactorReading::Level(0.0)), 0.0);
        assert_eq!(read(FactorReading::Level(0.5)), 90.0);
        assert_eq!(read(FactorReading::Level(0.05)), 100.0 * 4.5 / 20.0);
        assert_eq!(read(FactorReading::Floored), 100.0 * 19.0 / 20.0);
        // A non-finite reading has no position, however full the bucket.
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let session = day("2026-10-14");
            let factor = Factor::DebtToEquity;
            let level = FactorReading::Level(v);
            let read =
                bucket_position(&conn, "ASK", "Technology", CapBand::Mid, factor, level, session);
            assert_eq!(read.unwrap(), None, "{v}");
        }
    }
}
