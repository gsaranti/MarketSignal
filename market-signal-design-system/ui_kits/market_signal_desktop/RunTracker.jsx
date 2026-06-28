// RunTracker.jsx — the one run tracker, shared by all three jobs. Opens in
// place of the main content pane (never a modal takeover). The user can leave
// it — open a report from the sidebar — while the run keeps going; the footer
// returns them. Only the per-unit progress label differs per job kind:
//   report -> per-step, portfolio -> per-holding, trade -> per-cell.

const RUN_UNITS = {
  report: {
    title: "Generating this week's issue",
    unitLabel: "step",
    rows: [
      { name: "Ingest research inbox", status: "done" },
      { name: "Head analyst · first pass", status: "done" },
      { name: "Bull voice · stress-test", status: "done" },
      { name: "Bear voice · stress-test", status: "running" },
      { name: "Balanced voice · reconcile", status: "queued" },
      { name: "Retrospective · grade issue 140", status: "queued" },
      { name: "Synthesize · render Markdown", status: "queued" },
    ],
  },
  portfolio: {
    title: "Analyzing portfolio · 23 holdings",
    unitLabel: "holding",
    rows: [
      { name: "NVDA · grade + targets", status: "done" },
      { name: "ASML · grade + targets", status: "done" },
      { name: "XOM · grade + targets", status: "done" },
      { name: "VTI · reduced verdict", status: "running" },
      { name: "RXRX · evidence check", status: "queued" },
      { name: "Roll-up · construction panel", status: "queued" },
    ],
  },
  trade: {
    title: "Discovering opportunities · 3 × 3 matrix",
    unitLabel: "cell",
    rows: [
      { name: "High · short", status: "done" },
      { name: "High · mid", status: "done" },
      { name: "High · long", status: "done" },
      { name: "Medium · short", status: "running" },
      { name: "Medium · mid", status: "queued" },
      { name: "Low · short → long", status: "queued" },
      { name: "Calibration scorecard", status: "queued" },
    ],
  },
};

const STREAM_SAMPLE = {
  report: "…the Bear read cannot be fully refuted. Demand destruction is doing more work than the consensus expects; the 2014 analogue is more relevant than the 2007 one. We hold this next to the thesis as a permanent caveat rather than",
  portfolio: "…VTI graded on exposure, valuation, and house-view — no company-quality score applies to a broad index. Held as ballast at 16.2%, inside the 14–18% band. Action: hold. The reduced card is legitimate, not broken;",
  trade: "…Medium · short: VRT clears the gate on backlog inflection and the liquid-cooling attach rate. Conviction 4. Narrative and reality are converging mid-cycle. Bear: hyperscaler capex digestion pauses order flow. Entry matters more than",
};

function RunStatusPip({ status }) {
  const map = {
    done:    { ch: "\u2713", color: "var(--ana-up)" },
    running: { ch: "\u25B8", color: "var(--accent)" },
    queued:  { ch: "\u00B7", color: "var(--ink-3)" },
  };
  const m = map[status] || map.queued;
  return <span style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: m.color, width: 12, display: "inline-block", textAlign: "center" }}>{m.ch}</span>;
}

function RunTracker({ kind = "report", onLeave, onCancel }) {
  const cfg = RUN_UNITS[kind] || RUN_UNITS.report;
  const doneCount = cfg.rows.filter(r => r.status === "done").length;
  const runningIdx = cfg.rows.findIndex(r => r.status === "running");
  const pct = Math.round(((doneCount + 0.4) / cfg.rows.length) * 100);

  return (
    <div style={{ flex: 1, overflowY: "auto", background: "var(--paper)" }}>
      {/* toolbar */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "10px 32px", borderBottom: "1px solid var(--hairline)", background: "var(--paper)" }}>
        <AnaHead>Run tracker · {cfg.unitLabel} progress</AnaHead>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <button onClick={onLeave} style={{
            padding: "6px 11px", fontFamily: "var(--font-sans)", fontSize: 13, fontWeight: 500, whiteSpace: "nowrap",
            border: "1px solid var(--ink)", background: "transparent", color: "var(--ink)", cursor: "pointer", borderRadius: 2,
          }}>Leave — keeps running</button>
          <button onClick={onCancel} style={{
            padding: "6px 11px", fontFamily: "var(--font-sans)", fontSize: 13, fontWeight: 500, whiteSpace: "nowrap",
            border: "1px solid var(--hairline)", background: "transparent", color: "var(--ink-2)", cursor: "pointer", borderRadius: 2,
          }}>Cancel run</button>
        </div>
      </div>

      <div style={{ maxWidth: 820, margin: "0 auto", padding: "32px 32px 96px" }}>
        <h2 style={{ fontFamily: "var(--font-serif)", fontSize: 22, fontWeight: 600, color: "var(--ink)", margin: "0 0 4px" }}>{cfg.title}</h2>
        <p style={{ fontFamily: "var(--font-serif)", fontSize: 14, fontStyle: "italic", color: "var(--ink-3)", margin: "0 0 4px" }}>
          You can leave this view — open a report from the sidebar — and the run will keep going in the background.
        </p>

        {/* overall progress — a single 1px bar, no spinner */}
        <div style={{ display: "flex", alignItems: "center", gap: 14, margin: "20px 0 24px" }}>
          <div style={{ flex: 1, height: 1, background: "var(--hairline-soft)", position: "relative", overflow: "hidden" }}>
            <div style={{ position: "absolute", left: 0, top: 0, bottom: 0, width: pct + "%", background: "var(--ink)" }} />
          </div>
          <span style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: "var(--ink-3)", fontVariantNumeric: "tabular-nums" }}>{doneCount} / {cfg.rows.length} {cfg.unitLabel}s</span>
        </div>

        {/* one row per request */}
        <AnaCard>
          {cfg.rows.map((r, i) => (
            <div key={r.name} style={{
              display: "flex", alignItems: "center", gap: 12, padding: "10px 16px",
              borderBottom: i < cfg.rows.length - 1 ? "1px solid var(--hairline-soft)" : "none",
              background: r.status === "running" ? "var(--paper-soft)" : "transparent",
            }}>
              <RunStatusPip status={r.status} />
              <span style={{ flex: 1, fontFamily: "var(--font-sans)", fontSize: 13, color: r.status === "queued" ? "var(--ink-3)" : "var(--ink)", fontWeight: r.status === "running" ? 600 : 400 }}>{r.name}</span>
              <span style={{ fontFamily: "var(--font-sans)", fontSize: 10, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--ink-3)" }}>{r.status}</span>
            </div>
          ))}
        </AnaCard>

        {/* streamed model output */}
        <div style={{ marginTop: 20 }}>
          <AnaHead style={{ marginBottom: 8 }}>Streamed output · {cfg.rows[runningIdx]?.name || "—"}</AnaHead>
          <div style={{
            border: "1px solid var(--hairline)", borderRadius: 2, padding: "14px 16px", background: "var(--paper-edge)",
            fontFamily: "var(--font-mono)", fontSize: 12, lineHeight: 1.6, color: "var(--ink-2)",
          }}>
            {STREAM_SAMPLE[kind]}<span style={{ display: "inline-block", width: 7, height: 14, background: "var(--ink-2)", marginLeft: 2, verticalAlign: "text-bottom" }} />
          </div>
        </div>
      </div>
    </div>
  );
}

Object.assign(window, { RunTracker });
