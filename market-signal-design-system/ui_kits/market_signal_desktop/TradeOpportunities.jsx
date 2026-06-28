// TradeOpportunities.jsx — the 3 x 3 risk x horizon matrix (analytical
// register). Opportunity cards lead with a directional thesis; the leading
// metric is the visual spine. Since-flagged perf carries the restrained
// sparkline. Empty cells are honest, never errors.

const ARCHETYPE_LABEL = {
  "secular-compounder": "Secular compounder",
  "ai-infra": "AI infra",
  "commodity-cyclical": "Commodity cyclical",
  "disruptor": "Disruptor",
  "quality-compounder": "Quality compounder",
};
const STATUS_META = {
  "new":         { label: "New", color: "var(--accent)" },
  "still-valid": { label: "Still valid", color: "var(--ana-up)" },
  "played-out":  { label: "Played out", color: "var(--ink-3)" },
  "invalidated": { label: "Invalidated", color: "var(--ana-down)" },
};

function StatusDot({ status }) {
  const m = STATUS_META[status] || STATUS_META.new;
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
      <span style={{ width: 5, height: 5, borderRadius: "50%", background: m.color, flexShrink: 0 }} />
      <span style={{ fontFamily: "var(--font-sans)", fontSize: 10, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--ink-2)", whiteSpace: "nowrap" }}>{m.label}</span>
    </span>
  );
}

function SinceFlagged({ s }) {
  if (!s) {
    return (
      <div style={{ padding: "10px 14px", borderTop: "1px solid var(--hairline-soft)", background: "var(--paper-edge)" }}>
        <AnaHead style={{ color: "var(--ink-3)", marginBottom: 2 }}>Since flagged</AnaHead>
        <div style={{ fontFamily: "var(--font-serif)", fontSize: 12, fontStyle: "italic", color: "var(--ink-3)" }}>Debut — no track record yet.</div>
      </div>
    );
  }
  return (
    <div style={{ padding: "10px 14px", borderTop: "1px solid var(--hairline-soft)", background: "var(--paper-edge)" }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 10, marginBottom: 6 }}>
        <AnaHead style={{ color: "var(--ink-3)" }}>Since flagged · {s.windows}</AnaHead>
        <Sparkline data={s.curve} dir={s.return.dir} w={84} h={24} />
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "4px 12px" }}>
        <SF k="Return" v={<Dir dir={s.return.dir} size={12}>{s.return.val}</Dir>} />
        <SF k="vs sector" v={<Dir dir={s.vsSector.dir} size={12}>{s.vsSector.val}</Dir>} />
        <SF k="Max DD" v={<span style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: "var(--ink-2)" }}>{s.drawdown}</span>} />
        <SF k="Metric" v={<span style={{ fontFamily: "var(--font-sans)", fontSize: 11, color: s.continuation === "broken" ? "var(--ana-down)" : s.continuation === "watch" ? "var(--ink-2)" : "var(--ana-up)" }}>{s.continuation}</span>} />
      </div>
    </div>
  );
}
function SF({ k, v }) {
  return (
    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", gap: 8 }}>
      <span style={{ fontFamily: "var(--font-sans)", fontSize: 10, letterSpacing: "0.04em", textTransform: "uppercase", color: "var(--ink-3)" }}>{k}</span>
      {v}
    </div>
  );
}

function OpportunityCard({ o }) {
  return (
    <AnaCard style={{ display: "flex", flexDirection: "column" }}>
      {/* header */}
      <div style={{ padding: "12px 14px 10px", borderBottom: "1px solid var(--hairline-soft)" }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8, marginBottom: 6 }}>
          <span style={{ fontFamily: "var(--font-mono)", fontWeight: 500, fontSize: 15, letterSpacing: "0.02em", color: "var(--ink)" }}>{o.ticker}</span>
          <StatusDot status={o.status} />
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
          <span style={{ fontFamily: "var(--font-sans)", fontSize: 10, letterSpacing: "0.04em", textTransform: "uppercase", color: "var(--ink-2)", border: "1px solid var(--hairline)", borderRadius: 2, padding: "1px 5px" }}>{ARCHETYPE_LABEL[o.archetype]}</span>
          <span style={{ fontFamily: "var(--font-sans)", fontSize: 10, letterSpacing: "0.04em", textTransform: "uppercase", color: "var(--ink-3)" }}>{o.mode}</span>
        </div>
      </div>

      {/* directional thesis */}
      <div style={{ padding: "10px 14px" }}>
        <p style={{ fontFamily: "var(--font-serif)", fontSize: 13, lineHeight: 1.45, letterSpacing: "-0.006em", color: "var(--ink)", margin: 0 }}>{o.thesis}</p>
      </div>

      {/* leading metric — the spine, visually prominent */}
      <div style={{ margin: "0 14px 10px", padding: "8px 10px", border: "1px solid var(--hairline)", borderRadius: 2, background: "var(--paper-soft)" }}>
        <div style={{ fontFamily: "var(--font-sans)", fontSize: 10, letterSpacing: "0.04em", textTransform: "uppercase", color: "var(--ink-3)", lineHeight: 1.3 }}>{o.metric.label}</div>
        <div style={{ display: "flex", alignItems: "baseline", gap: 8, marginTop: 6 }}>
          <span style={{ fontFamily: "var(--font-mono)", fontSize: 22, fontWeight: 500, color: "var(--ink)", fontVariantNumeric: "tabular-nums", letterSpacing: "-0.01em" }}>{o.metric.val}</span>
          <Dir dir={o.metric.trend} size={11}></Dir>
        </div>
      </div>

      {/* catalyst + conviction */}
      <div style={{ padding: "0 14px 10px" }}>
        <div style={{ display: "flex", gap: 8, marginBottom: 8 }}>
          <AnaHead style={{ color: "var(--ink-3)", whiteSpace: "nowrap", marginTop: 1 }}>Catalyst</AnaHead>
          <span style={{ fontFamily: "var(--font-serif)", fontSize: 12, lineHeight: 1.4, color: "var(--ink-2)" }}>{o.catalyst}</span>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <AnaHead style={{ color: "var(--ink-3)" }}>Conviction</AnaHead>
          <Conviction value={o.conviction} />
        </div>
      </div>

      {/* narrative vs reality + bear (always present) */}
      <div style={{ padding: "0 14px 10px" }}>
        <div style={{ marginBottom: 6 }}>
          <AnaHead style={{ color: "var(--ink-3)", marginBottom: 2 }}>Narrative vs reality</AnaHead>
          <span style={{ fontFamily: "var(--font-serif)", fontSize: 12, lineHeight: 1.4, color: "var(--ink-2)" }}>{o.narrative}</span>
        </div>
        <div>
          <AnaHead style={{ color: "var(--ana-down)", marginBottom: 2 }}>Bear case</AnaHead>
          <span style={{ fontFamily: "var(--font-serif)", fontSize: 12, lineHeight: 1.4, color: "var(--ink-2)" }}>{o.bear}</span>
        </div>
      </div>

      {/* reveal: falsifiers, lineage, tech, entry, flags */}
      <div style={{ padding: "0 14px 10px" }}>
        <Reveal label="Falsifiers · lineage · entry">
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <RevRow k="Key falsifiers" v={o.falsifiers} />
            {o.tech && <RevRow k="Technology read" v={o.tech} />}
            <RevRow k="Entry" v={o.entry} />
            <RevRow k="Risk / forensic" v={o.flags} />
            <RevRow k="Lineage" v={"world-change → mechanism → node → metric"} mono />
          </div>
        </Reveal>
      </div>

      <SinceFlagged s={o.since} />
    </AnaCard>
  );
}
function RevRow({ k, v, mono }) {
  return (
    <div>
      <AnaHead style={{ color: "var(--ink-3)", marginBottom: 2 }}>{k}</AnaHead>
      <span style={{ fontFamily: mono ? "var(--font-mono)" : "var(--font-serif)", fontSize: mono ? 11 : 12, lineHeight: 1.4, color: "var(--ink-2)" }}>{v}</span>
    </div>
  );
}

function EmptyCell() {
  return (
    <div style={{
      border: "1px dashed var(--hairline)", borderRadius: 2, minHeight: 88,
      display: "flex", alignItems: "center", justifyContent: "center", padding: 14,
    }}>
      <span style={{ fontFamily: "var(--font-serif)", fontSize: 12, fontStyle: "italic", color: "var(--ink-3)", textAlign: "center" }}>Nothing qualified this run.</span>
    </div>
  );
}

function CalibrationScorecard({ c }) {
  return (
    <AnaCard style={{ marginTop: 24 }}>
      <div style={{ padding: "12px 18px", borderBottom: "1px solid var(--hairline-soft)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <AnaHead>Calibration scorecard</AnaHead>
          {c.shadow && <span style={{ fontFamily: "var(--font-sans)", fontSize: 10, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--ink-2)", border: "1px solid var(--hairline)", borderRadius: 2, padding: "1px 6px" }}>Shadow · not yet steering</span>}
        </div>
      </div>
      <KeyFigureStrip items={[
        { label: "Picks", value: c.picks },
        { label: "Matured", value: c.matured },
        { label: "Hit rate", value: c.hitRate },
        { label: "Avg return", value: <Dir dir={c.avgReturn.dir} size={16}>{c.avgReturn.val}</Dir> },
        { label: "vs benchmark", value: <Dir dir={c.vsBench.dir} size={16}>{c.vsBench.val}</Dir> },
      ]} style={{ border: 0, borderRadius: 0 }} />
      <div style={{ padding: "12px 18px", borderTop: "1px solid var(--hairline-soft)" }}>
        <AnaHead style={{ marginBottom: 4 }}>Failure modes</AnaHead>
        <p style={{ fontFamily: "var(--font-serif)", fontSize: 13, lineHeight: 1.5, color: "var(--ink-2)", margin: 0, maxWidth: "78ch" }}>{c.failures}</p>
      </div>
    </AnaCard>
  );
}

function TOToolbar() {
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "10px 32px", borderBottom: "1px solid var(--hairline)", background: "var(--paper)" }}>
      <AnaHead>Trade opportunities</AnaHead>
      <button style={{
        display: "inline-flex", alignItems: "center", gap: 6, padding: "7px 12px", whiteSpace: "nowrap",
        fontFamily: "var(--font-sans)", fontSize: 13, fontWeight: 500,
        border: "1px solid var(--ink)", background: "var(--ink)", color: "var(--paper)",
        cursor: "pointer", borderRadius: 2,
      }}>Run discovery</button>
    </div>
  );
}

const RISK_ROWS = [
  { key: "high", label: "High risk" },
  { key: "medium", label: "Medium risk" },
  { key: "low", label: "Low risk" },
];
const HORIZONS = [
  { key: "short", label: "Short term" },
  { key: "mid", label: "Mid term" },
  { key: "long", label: "Long term" },
];

function TradeOpportunities() {
  const { MATRIX, OPP, CALIBRATION } = window.MS_DATA;
  const total = RISK_ROWS.reduce((a, r) => a + HORIZONS.reduce((b, h) => b + (MATRIX[r.key][h.key]?.length || 0), 0), 0);

  return (
    <div style={{ flex: 1, overflowY: "auto", background: "var(--paper)" }}>
      <TOToolbar />
      <div style={{ maxWidth: 1100, margin: "0 auto", padding: "20px 28px 96px" }}>
        {/* shadow / calibration banner */}
        {CALIBRATION.shadow && (
          <div style={{ display: "flex", gap: 12, alignItems: "baseline", padding: "10px 14px", border: "1px solid var(--hairline)", borderRadius: 2, marginBottom: 20, background: "var(--paper)" }}>
            <AnaHead style={{ color: "var(--ink)", whiteSpace: "nowrap" }}>Shadow run</AnaHead>
            <span style={{ fontFamily: "var(--font-serif)", fontSize: 13, fontStyle: "italic", lineHeight: 1.45, color: "var(--ink-2)" }}>
              Early runs are calibration. The scorecard below is shown honestly but is not yet steering which ideas surface.
            </span>
          </div>
        )}

        {/* matrix header */}
        <div style={{ display: "grid", gridTemplateColumns: "96px repeat(3, 1fr)", gap: 12, marginBottom: 8, alignItems: "end" }}>
          <div></div>
          {HORIZONS.map(h => (
            <AnaHead key={h.key} style={{ paddingLeft: 2 }}>{h.label}</AnaHead>
          ))}
        </div>

        {/* matrix rows */}
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          {RISK_ROWS.map(r => (
            <div key={r.key} style={{ display: "grid", gridTemplateColumns: "96px repeat(3, 1fr)", gap: 12, alignItems: "start" }}>
              <div style={{ paddingTop: 4 }}>
                <div style={{ fontFamily: "var(--font-sans)", fontSize: 13, fontWeight: 600, color: "var(--ink)", letterSpacing: "0.01em" }}>{r.label}</div>
                <div style={{ height: 2, width: 24, background: "var(--accent)", marginTop: 6, opacity: 0.85 }} />
              </div>
              {HORIZONS.map(h => {
                const tickers = MATRIX[r.key][h.key] || [];
                return (
                  <div key={h.key} style={{ display: "flex", flexDirection: "column", gap: 12, minWidth: 0 }}>
                    {tickers.length === 0
                      ? <EmptyCell />
                      : tickers.map(t => <OpportunityCard key={t} o={OPP[t]} />)}
                  </div>
                );
              })}
            </div>
          ))}
        </div>

        {total === 0 && (
          <div style={{ padding: "40px 0", textAlign: "center", fontFamily: "var(--font-serif)", fontSize: 15, fontStyle: "italic", color: "var(--ink-3)" }}>
            Nothing qualified this run. The gates held; that is a result, not a failure.
          </div>
        )}

        <CalibrationScorecard c={CALIBRATION} />
      </div>
    </div>
  );
}

Object.assign(window, { TradeOpportunities, OpportunityCard, CalibrationScorecard });
