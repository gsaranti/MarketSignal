// Portfolio.jsx — Portfolio Analysis surface (analytical register).
// Two-step trigger (Pull holdings -> Run analysis), holdings as cards
// (full / reduced / not-rated / insufficient), and a whole-book roll-up.
// Layout/IA per the brief; restyle, not re-architect.

// ---- Standing-thesis anchor — handles a long thesis with graceful overflow ----
function ThesisAnchor({ text, lead = true }) {
  const [open, setOpen] = React.useState(false);
  const [overflows, setOverflows] = React.useState(false);
  const ref = React.useRef(null);
  React.useLayoutEffect(() => {
    const el = ref.current;
    if (el) setOverflows(el.scrollHeight - el.clientHeight > 2);
  }, [text]);
  return (
    <div>
      <AnaHead style={{ marginBottom: 6 }}>Standing thesis</AnaHead>
      <p ref={ref} style={{
        fontFamily: "var(--font-serif)",
        fontSize: lead ? 15 : 14, lineHeight: 1.5, letterSpacing: "-0.006em",
        color: "var(--ink)", margin: 0,
        ...(!open ? {
          display: "-webkit-box", WebkitLineClamp: 3, WebkitBoxOrient: "vertical",
          overflow: "hidden",
        } : {}),
      }}>{text}</p>
      {(overflows || open) && (
        <button onClick={() => setOpen(o => !o)} style={{
          marginTop: 6, background: "transparent", border: 0, padding: 0, cursor: "pointer",
          fontFamily: "var(--font-sans)", fontSize: 11, fontWeight: 600,
          letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--accent)",
        }}>{open ? "Show less" : "Read full thesis"}</button>
      )}
    </div>
  );
}

function SubScores({ sub }) {
  const entries = Object.entries(sub);
  const labelMap = { quality: "Qual", valuation: "Val", momentum: "Mom", risk: "Risk", exposure: "Expo", houseView: "House" };
  return (
    <div style={{ display: "grid", gridTemplateColumns: `repeat(${entries.length}, 1fr)`, gap: "0 4px" }}>
      {entries.map(([k, v]) => (
        <div key={k}>
          <div style={{ fontSize: 9, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--ink-3)", marginBottom: 4 }}>{labelMap[k] || k}</div>
          <Grade value={v} size="sm" />
        </div>
      ))}
    </div>
  );
}

function KV({ rows }) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "max-content 1fr", columnGap: 14, rowGap: 5,
      fontFamily: "var(--font-sans)", fontSize: 12 }}>
      {rows.map((r, i) => (
        <React.Fragment key={i}>
          <div style={{ color: "var(--ink-3)", whiteSpace: "nowrap" }}>{r.k}</div>
          <div style={{ color: "var(--ink)" }}>{r.v}</div>
        </React.Fragment>
      ))}
    </div>
  );
}

function Scenarios({ rows }) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", borderTop: "1px solid var(--hairline-soft)" }}>
      {rows.map((s, i) => (
        <div key={s.k} style={{ padding: "10px 14px", borderRight: i < 2 ? "1px solid var(--hairline-soft)" : "none" }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", marginBottom: 4 }}>
            <AnaHead style={{ color: "var(--ink-2)" }}>{s.k}</AnaHead>
            <span style={{ fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--ink-3)" }}>{s.p}</span>
          </div>
          <div style={{ fontFamily: "var(--font-mono)", fontSize: 14, color: "var(--ink)", fontVariantNumeric: "tabular-nums" }}>{s.t}</div>
          <div style={{ fontFamily: "var(--font-serif)", fontSize: 12, lineHeight: 1.4, color: "var(--ink-3)", marginTop: 3 }}>{s.note}</div>
        </div>
      ))}
    </div>
  );
}

function ClassTag({ klass, state }) {
  const map = {
    "rated": "Stock · full verdict",
    "rated-reduced": "ETF · reduced verdict",
    "not-rated": "Not rated",
    "insufficient": "Insufficient evidence",
  };
  const klassMap = { stock: "Stock", etf: "ETF / fund", option: "Options", cash: "Cash", unsupported: "Unsupported" };
  return (
    <AnaHead style={{ color: "var(--ink-3)", whiteSpace: "nowrap" }}>
      {state === "rated" ? map.rated : state === "rated-reduced" ? map["rated-reduced"] : (klassMap[klass] || klass) + " · " + map[state]}
    </AnaHead>
  );
}

function HoldingCard({ h }) {
  // Not-rated / insufficient — short reason, no grade, reads as legitimately reduced.
  if (h.state === "not-rated" || h.state === "insufficient") {
    const abst = h.state === "insufficient";
    return (
      <AnaCard style={{ borderColor: "var(--hairline)", background: abst ? "var(--paper)" : "var(--paper)" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 16, padding: "14px 18px" }}>
          <div style={{ minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "baseline", gap: 10, marginBottom: 6 }}>
              <span style={{ fontFamily: "var(--font-mono)", fontWeight: 500, fontSize: 15, letterSpacing: "0.02em", color: "var(--ink)" }}>{h.ticker}</span>
              <ClassTag klass={h.klass} state={h.state} />
            </div>
            <p style={{ fontFamily: "var(--font-serif)", fontSize: 13, lineHeight: 1.45, color: "var(--ink-2)", margin: 0, maxWidth: "70ch" }}>{h.reason}</p>
          </div>
          <div style={{ textAlign: "right", flexShrink: 0 }}>
            <AnaHead style={{ color: "var(--ink-3)", marginBottom: 3 }}>Weight</AnaHead>
            <span style={{ fontFamily: "var(--font-mono)", fontSize: 14, color: "var(--ink-2)", fontVariantNumeric: "tabular-nums" }}>{h.weight}</span>
          </div>
        </div>
      </AnaCard>
    );
  }

  const reduced = h.state === "rated-reduced";
  return (
    <AnaCard>
      {/* header */}
      <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 16, padding: "16px 18px 14px", borderBottom: "1px solid var(--hairline-soft)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}>
          <Grade value={h.grade} size="lg" />
          <div style={{ minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
              <span style={{ fontFamily: "var(--font-mono)", fontWeight: 500, fontSize: 15, letterSpacing: "0.02em", color: "var(--ink)" }}>{h.ticker}</span>
              <ClassTag klass={h.klass} state={h.state} />
            </div>
            <div style={{ fontFamily: "var(--font-sans)", fontSize: 12, color: "var(--ink-3)", marginTop: 1 }}>{h.name} · {h.sector}</div>
          </div>
        </div>
        <div style={{ textAlign: "right", flexShrink: 0 }}>
          <AnaHead style={{ color: "var(--ink-3)", marginBottom: 3 }}>Unrealized</AnaHead>
          <Dir dir={h.unrealized.dir} size={16}>{h.unrealized.val}</Dir>
        </div>
      </div>

      {/* thesis anchor */}
      <div style={{ padding: "14px 18px", borderBottom: "1px solid var(--hairline-soft)" }}>
        <ThesisAnchor text={h.thesis} />
      </div>

      {/* two linked blocks */}
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr" }}>
        <div style={{ padding: "14px 18px", borderRight: "1px solid var(--hairline-soft)" }}>
          <AnaHead style={{ marginBottom: 10 }}>{reduced ? "Intrinsic verdict · reduced" : "Intrinsic verdict"}</AnaHead>
          <div style={{ marginBottom: 12 }}><SubScores sub={h.sub} /></div>
          <KV rows={[
            { k: "Conviction", v: <Conviction value={h.conviction} /> },
            ...(reduced ? [] : [
              { k: "EOM target", v: <span style={{ fontFamily: "var(--font-mono)", fontVariantNumeric: "tabular-nums" }}>{h.eom} <Methodology note="End-of-month target: DCF fair value bridged to a 1-month multiple path. Deterministic; same inputs, same number." /></span> },
              { k: "EOY target", v: <span style={{ fontFamily: "var(--font-mono)", fontVariantNumeric: "tabular-nums" }}>{h.eoy}</span> },
            ]),
            { k: "Standalone", v: h.standalone },
          ]} />
          {h.health && <div style={{ fontFamily: "var(--font-serif)", fontSize: 12, lineHeight: 1.45, color: "var(--ink-3)", marginTop: 10 }}>{h.health}</div>}
        </div>

        <div style={{ padding: "14px 18px" }}>
          <AnaHead style={{ marginBottom: 10 }}>Portfolio action</AnaHead>
          <div style={{ display: "flex", alignItems: "baseline", gap: 8, marginBottom: 10 }}>
            <span style={{ fontFamily: "var(--font-sans)", fontSize: 15, fontWeight: 600, color: "var(--ink)" }}>{h.action}</span>
            <AnaHead style={{ color: "var(--ink-3)", whiteSpace: "nowrap" }}>to {h.targetWeight}</AnaHead>
          </div>
          <div style={{ marginBottom: 10 }}>
            <KV rows={[
              { k: "Weight", v: <span style={{ fontFamily: "var(--font-mono)", fontVariantNumeric: "tabular-nums" }}>{h.weight}</span> },
              { k: "Est. adj.", v: <span style={{ fontFamily: "var(--font-mono)", fontVariantNumeric: "tabular-nums" }}>{h.adj}</span> },
            ]} />
          </div>
          <p style={{ fontFamily: "var(--font-serif)", fontSize: 13, lineHeight: 1.45, letterSpacing: "-0.006em", color: "var(--ink-2)", margin: 0 }}>{h.rationale}</p>
        </div>
      </div>

      {/* scenarios */}
      {h.scenarios && <Scenarios rows={h.scenarios} />}

      {/* reveal: triggers + dead-money flag */}
      {(h.triggers || h.deadMoney) && (
        <div style={{ padding: "12px 18px", borderTop: "1px solid var(--hairline-soft)" }}>
          {h.deadMoney && (
            <div style={{ display: "flex", gap: 8, marginBottom: h.triggers ? 10 : 0 }}>
              <AnaHead style={{ color: "var(--ana-down)", whiteSpace: "nowrap" }}>Capital-efficiency</AnaHead>
              <span style={{ fontFamily: "var(--font-serif)", fontSize: 12, color: "var(--ink-2)" }}>{h.deadMoney}</span>
            </div>
          )}
          {h.triggers && (
            <Reveal label="Triggers & falsifiers">
              <KV rows={[
                { k: "Add", v: h.triggers.add },
                { k: "Trim", v: h.triggers.trim },
                { k: "Sell", v: h.triggers.sell },
              ]} />
            </Reveal>
          )}
        </div>
      )}

      {/* what changed + sparkline */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 16, padding: "12px 18px", borderTop: "1px solid var(--hairline-soft)", background: "var(--paper-edge)" }}>
        <div>
          <AnaHead style={{ color: "var(--ink-3)", marginBottom: 3 }}>What changed · since last run</AnaHead>
          <div style={{ fontFamily: "var(--font-sans)", fontSize: 12, color: "var(--ink-2)" }}>
            Intrinsic <span style={{ color: "var(--ink)" }}>{h.changed.intrinsic}</span> · action <span style={{ color: "var(--ink)" }}>{h.changed.action}</span> · position <span style={{ color: "var(--ink)" }}>{h.changed.position}</span>
          </div>
        </div>
        {h.curve && <Sparkline data={h.curve} dir={h.unrealized.dir} />}
      </div>
    </AnaCard>
  );
}

// ---- Whole-book roll-up & construction panel ----
function ConstructionPanel({ book }) {
  return (
    <AnaCard style={{ marginTop: 24 }}>
      <div style={{ padding: "14px 18px", borderBottom: "1px solid var(--hairline-soft)" }}>
        <AnaHead style={{ marginBottom: 8 }}>Roll-up & construction · whole book</AnaHead>
        <div style={{ display: "flex", gap: 8, alignItems: "baseline" }}>
          <span style={{ fontFamily: "var(--font-sans)", fontSize: 15, fontWeight: 600, color: "var(--ink)" }}>Risk posture: {book.posture}</span>
        </div>
        <p style={{ fontFamily: "var(--font-serif)", fontSize: 13, lineHeight: 1.5, color: "var(--ink-2)", margin: "8px 0 0", maxWidth: "78ch" }}>
          <span style={{ fontWeight: 600, color: "var(--ink)" }}>Cash & deployment.</span> {book.cash}
        </p>
      </div>

      {/* concentration grid */}
      <div style={{ padding: "12px 18px", borderBottom: "1px solid var(--hairline-soft)" }}>
        <AnaHead style={{ marginBottom: 8 }}>Concentration & exposure</AnaHead>
        <table style={{ width: "100%", borderCollapse: "collapse" }}>
          <thead>
            <tr>
              {["Cluster", "Weight", "Names", "β-contrib", "Δ run"].map((c, i) => (
                <th key={c} style={{
                  fontFamily: "var(--font-sans)", fontSize: 11, fontWeight: 600, letterSpacing: "0.05em",
                  textTransform: "uppercase", color: "var(--ink-3)", textAlign: i === 0 ? "left" : "right",
                  padding: "6px 8px", borderBottom: "1px solid var(--hairline)", whiteSpace: "nowrap",
                }}>{c}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {book.concentration.map((r, i) => (
              <tr key={r.cluster}>
                <td style={{ fontSize: 12, color: "var(--ink)", padding: "6px 8px", borderBottom: i < book.concentration.length - 1 ? "1px solid var(--hairline-soft)" : "none" }}>{r.cluster}</td>
                <td style={tdNum(i, book.concentration.length)}>{r.weight}</td>
                <td style={tdNum(i, book.concentration.length)}>{r.names}</td>
                <td style={tdNum(i, book.concentration.length)}>{r.beta}</td>
                <td style={{ ...tdNum(i, book.concentration.length), textAlign: "right" }}><Dir dir={r.delta.dir}>{r.delta.val}</Dir></td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* overlap clusters */}
      <div style={{ padding: "12px 18px", borderBottom: "1px solid var(--hairline-soft)" }}>
        <AnaHead style={{ marginBottom: 8 }}>Overlap clusters</AnaHead>
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          {book.overlap.map(o => (
            <div key={o.name}>
              <div style={{ display: "flex", gap: 10, alignItems: "baseline", flexWrap: "wrap" }}>
                <span style={{ fontFamily: "var(--font-sans)", fontSize: 13, fontWeight: 600, color: "var(--ink)" }}>{o.name}</span>
                <span style={{ fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--ink-3)" }}>{o.holdings}</span>
              </div>
              <div style={{ fontFamily: "var(--font-serif)", fontSize: 12, lineHeight: 1.45, color: "var(--ink-2)", marginTop: 2 }}>{o.note}</div>
            </div>
          ))}
        </div>
      </div>

      {/* closed positions + not-rated risk */}
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr" }}>
        <div style={{ padding: "12px 18px", borderRight: "1px solid var(--hairline-soft)" }}>
          <AnaHead style={{ marginBottom: 8 }}>Positions closed since last run</AnaHead>
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            {book.closed.map(c => (
              <div key={c.ticker} style={{ display: "flex", gap: 10 }}>
                <span style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: "var(--ink)", minWidth: 36 }}>{c.ticker}</span>
                <span style={{ fontFamily: "var(--font-serif)", fontSize: 12, lineHeight: 1.45, color: "var(--ink-2)" }}>{c.note}</span>
              </div>
            ))}
          </div>
        </div>
        <div style={{ padding: "12px 18px" }}>
          <AnaHead style={{ marginBottom: 8 }}>Not-rated risk contribution</AnaHead>
          <p style={{ fontFamily: "var(--font-serif)", fontSize: 12, lineHeight: 1.45, color: "var(--ink-2)", margin: 0 }}>{book.notRatedRisk}</p>
        </div>
      </div>
    </AnaCard>
  );
}
function tdNum(i, len) {
  return { fontFamily: "var(--font-mono)", fontSize: 12, fontVariantNumeric: "tabular-nums", color: "var(--ink)",
    textAlign: "right", padding: "6px 8px", borderBottom: i < len - 1 ? "1px solid var(--hairline-soft)" : "none" };
}

// ---- Trigger controls (two-step) ----
function PortfolioToolbar({ phase, onPull, onRun }) {
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "10px 32px", borderBottom: "1px solid var(--hairline)", background: "var(--paper)" }}>
      <AnaHead>Portfolio analysis</AnaHead>
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <button onClick={onPull} style={btnStyle(phase !== "empty")}>1 · Pull holdings</button>
        <span style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: "var(--ink-3)" }}>→</span>
        <button onClick={onRun} disabled={phase === "empty"} style={btnStyle(phase === "pulled", phase === "empty")}>2 · Run analysis</button>
      </div>
    </div>
  );
}
function btnStyle(primary, disabled) {
  return {
    display: "inline-flex", alignItems: "center", gap: 6, padding: "7px 12px",
    fontFamily: "var(--font-sans)", fontSize: 13, fontWeight: 500, whiteSpace: "nowrap",
    border: "1px solid " + (disabled ? "var(--hairline)" : "var(--ink)"),
    background: primary && !disabled ? "var(--ink)" : "transparent",
    color: disabled ? "var(--ink-3)" : (primary ? "var(--paper)" : "var(--ink)"),
    cursor: disabled ? "default" : "pointer", borderRadius: 2,
    transition: "all 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
  };
}

function EmptyPortfolio({ phase, onPull, onRun }) {
  const pulled = phase === "pulled";
  return (
    <div style={{ maxWidth: 720, margin: "0 auto", padding: "72px 32px" }}>
      <h2 style={{ fontFamily: "var(--font-serif)", fontSize: 22, fontWeight: 600, color: "var(--ink)", margin: "0 0 8px" }}>
        {pulled ? "23 holdings pulled. Not yet analyzed." : "No holdings pulled yet."}
      </h2>
      <p style={{ fontFamily: "var(--font-serif)", fontSize: 15, lineHeight: 1.55, color: "var(--ink-2)", margin: "0 0 24px", maxWidth: "60ch" }}>
        {pulled
          ? "Holdings were fetched from your connected Schwab account. Run the analysis to grade them; nothing is graded until you ask."
          : "Holdings are fetched only on explicit action — never auto-synced. Pull from your connected Schwab account, then run the analysis."}
      </p>
      <div style={{ display: "flex", gap: 10, marginBottom: 32 }}>
        <button onClick={onPull} style={btnStyle(!pulled)}>Pull holdings</button>
        <button onClick={onRun} disabled={!pulled} style={btnStyle(pulled, !pulled)}>Run analysis</button>
      </div>
      <div style={{ borderTop: "1px solid var(--hairline)", paddingTop: 16, maxWidth: "60ch" }}>
        <AnaHead style={{ marginBottom: 6 }}>Supplement · manual import</AnaHead>
        <p style={{ fontFamily: "var(--font-serif)", fontSize: 13, lineHeight: 1.5, color: "var(--ink-3)", margin: 0 }}>
          Paste symbols, quantities, and cost bases — or drop a CSV — to add positions Schwab does not report. This supplements the pull; it does not replace the Schwab connection, which gates the job regardless.
        </p>
      </div>
    </div>
  );
}

function Portfolio() {
  const { BOOK, HOLDINGS } = window.MS_DATA;
  const [phase, setPhase] = React.useState("ran"); // empty | pulled | ran

  if (phase !== "ran") {
    return (
      <div style={{ flex: 1, overflowY: "auto", background: "var(--paper)" }}>
        <PortfolioToolbar phase={phase} onPull={() => setPhase("pulled")} onRun={() => setPhase("ran")} />
        <EmptyPortfolio phase={phase} onPull={() => setPhase("pulled")} onRun={() => setPhase("ran")} />
      </div>
    );
  }

  return (
    <div style={{ flex: 1, overflowY: "auto", background: "var(--paper)" }}>
      <PortfolioToolbar phase={phase} onPull={() => setPhase("pulled")} onRun={() => setPhase("ran")} />
      <div style={{ maxWidth: 980, margin: "0 auto", padding: "24px 32px 96px" }}>
        <KeyFigureStrip items={[
          { label: "Book value", value: BOOK.value },
          { label: "Holdings", value: BOOK.holdings },
          { label: "Rated", value: BOOK.rated },
          { label: "Not rated", value: BOOK.notRated },
          { label: "Since last run", value: <Dir dir={BOOK.sinceRun.dir} size={16}>{BOOK.sinceRun.val}</Dir> },
          { label: "Posture", value: BOOK.posture, sans: true },
        ]} style={{ marginBottom: 24 }} />

        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          {HOLDINGS.map(h => <HoldingCard key={h.ticker} h={h} />)}
        </div>

        <ConstructionPanel book={BOOK} />
      </div>
    </div>
  );
}

Object.assign(window, { Portfolio, HoldingCard, ConstructionPanel, ThesisAnchor });
