// LatestReport.jsx — The loosest, most generous surface in the system.
// A single readable column ~64ch, serif body, 8px baseline rhythm,
// hairlines between sections, watchlist + retrospective insets, and
// the rare display moments (title + dateline) at restrained sizes.

function ReportToolbar({ title, dateline }) {
  const [hover, setHover] = React.useState(null);
  const btnStyle = (key, primary) => ({
    display: "inline-flex", alignItems: "center", gap: 6,
    padding: "7px 12px",
    fontFamily: "var(--font-sans)",
    fontSize: 13, fontWeight: 500,
    whiteSpace: "nowrap",
    border: "1px solid " + (primary ? "var(--ink)" : "var(--hairline)"),
    background: primary
      ? (hover === key ? "#2B241B" : "var(--ink)")
      : (hover === key ? "var(--paper-soft)" : "transparent"),
    color: primary ? "var(--paper)" : "var(--ink)",
    cursor: "pointer", borderRadius: 2,
    transition: "all 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
  });
  return (
    <div style={{
      display: "flex", alignItems: "center", justifyContent: "space-between",
      padding: "10px 32px",
      borderBottom: "1px solid var(--hairline)",
      background: "var(--paper)",
    }}>
      <div style={{
        fontFamily: "var(--font-sans)",
        fontSize: 11, letterSpacing: "0.05em", textTransform: "uppercase",
        color: "var(--ink-3)",
      }}>Latest report</div>
      <div style={{ display: "flex", gap: 8 }}>
        <button
          onMouseEnter={() => setHover("pdf")}
          onMouseLeave={() => setHover(null)}
          style={btnStyle("pdf", false)}>
          <Icon name="export_" size={13} />
          Export PDF
        </button>
        <button
          onMouseEnter={() => setHover("share")}
          onMouseLeave={() => setHover(null)}
          style={btnStyle("share", false)}>
          <Icon name="file" size={13} />
          Share as Markdown
        </button>
      </div>
    </div>
  );
}

function SectionRule() {
  return (
    <div style={{
      position: "relative", textAlign: "center",
      margin: "44px 0", color: "var(--ink-3)",
      fontFamily: "var(--font-serif)", fontSize: 14, lineHeight: 1,
    }}>
      <span style={{
        display: "inline-block", padding: "0 14px",
        background: "var(--paper)", position: "relative", zIndex: 1,
      }}>✻</span>
      <div style={{
        position: "absolute", left: 0, right: 0, top: "50%",
        borderTop: "1px solid var(--hairline)",
      }} />
    </div>
  );
}

function Figure({ caption, source, height = 160, children }) {
  return (
    <figure style={{
      margin: "32px 0", padding: 0,
      border: "1px solid var(--hairline)",
      borderRadius: 2, background: "var(--paper)",
    }}>
      <div style={{ height, padding: 16 }}>{children}</div>
      <figcaption style={{
        borderTop: "1px solid var(--hairline-soft)",
        padding: "8px 16px",
        display: "flex", justifyContent: "space-between", gap: 16,
      }}>
        <span style={{
          fontFamily: "var(--font-sans)",
          fontSize: 11, letterSpacing: "0.05em", textTransform: "uppercase",
          color: "var(--ink-2)",
        }}>{caption}</span>
        <span style={{
          fontFamily: "var(--font-sans)",
          fontSize: 11, letterSpacing: "0.05em", textTransform: "uppercase",
          color: "var(--ink-3)",
        }}>Source · {source}</span>
      </figcaption>
    </figure>
  );
}

// A restrained SVG figure — single ink color, hairline grid, one accent
// band when emphasis is needed. Not a dashboard widget.
function YieldChart() {
  // Two series, 26 points each
  const series1 = [3.62, 3.71, 3.68, 3.79, 3.88, 3.92, 4.01, 3.98, 4.05, 4.12, 4.21, 4.18, 4.24, 4.31, 4.28, 4.36, 4.41, 4.38, 4.45, 4.48, 4.42, 4.39, 4.34, 4.36, 4.29, 4.31];
  const series2 = [4.41, 4.48, 4.52, 4.57, 4.60, 4.62, 4.65, 4.66, 4.69, 4.71, 4.72, 4.71, 4.70, 4.69, 4.67, 4.66, 4.64, 4.62, 4.66, 4.69, 4.72, 4.74, 4.71, 4.73, 4.70, 4.69];
  const W = 720, H = 130, P = 8;
  const min = 3.5, max = 4.9;
  const x = i => P + i * ((W - 2*P) / (series1.length - 1));
  const y = v => P + (1 - (v - min) / (max - min)) * (H - 2*P);
  const path = (s) => s.map((v, i) => (i === 0 ? "M" : "L") + x(i) + " " + y(v)).join(" ");
  return (
    <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
      {/* hairline grid */}
      {[0,1,2,3].map(i => (
        <line key={i} x1="0" x2={W} y1={P + i * (H - 2*P) / 3} y2={P + i * (H - 2*P) / 3}
          stroke="#DCD4C0" strokeWidth="0.5" />
      ))}
      {/* 2Y in ink */}
      <path d={path(series2)} stroke="#1F1A14" strokeWidth="1.25" fill="none" />
      {/* 10Y in accent oxblood — the single emphasized series */}
      <path d={path(series1)} stroke="#6E2230" strokeWidth="1.25" fill="none" />
      {/* y-axis labels (tabular, mono) */}
      <text x={W - 4} y={y(4.7) + 3}  textAnchor="end" fontFamily="IBM Plex Mono" fontSize="9" fill="#7A6F5F">4.70</text>
      <text x={W - 4} y={y(4.0) + 3}  textAnchor="end" fontFamily="IBM Plex Mono" fontSize="9" fill="#7A6F5F">4.00</text>
      <text x={W - 4} y={y(3.6) + 3}  textAnchor="end" fontFamily="IBM Plex Mono" fontSize="9" fill="#7A6F5F">3.60</text>
      {/* end labels */}
      <text x={x(25) - 4} y={y(series1[25]) - 5} textAnchor="end" fontFamily="Public Sans" fontSize="10" fill="#6E2230">10Y · 4.31%</text>
      <text x={x(25) - 4} y={y(series2[25]) + 12} textAnchor="end" fontFamily="Public Sans" fontSize="10" fill="#1F1A14">2Y · 4.69%</text>
    </svg>
  );
}

function Watchlist() {
  const { WATCHLIST } = window.MS_DATA;
  return (
    <div style={{
      margin: "32px 0",
      border: "1px solid var(--hairline)",
      borderRadius: 2, background: "var(--paper)",
    }}>
      <div style={{
        display: "flex", justifyContent: "space-between",
        padding: "10px 14px",
        borderBottom: "1px solid var(--hairline-soft)",
      }}>
        <span style={{
          fontFamily: "var(--font-sans)",
          fontSize: 11, letterSpacing: "0.05em", textTransform: "uppercase",
          color: "var(--ink-2)",
        }}>Watchlist · this week</span>
        <span style={{
          fontFamily: "var(--font-sans)",
          fontSize: 11, letterSpacing: "0.05em", textTransform: "uppercase",
          color: "var(--ink-3)",
        }}>Close · Fri Mar 29</span>
      </div>
      <div style={{ padding: "4px 14px 8px 14px" }}>
        <div style={{
          display: "grid",
          gridTemplateColumns: "1fr 100px 100px 100px",
          gap: 0,
        }}>
          <div style={tHeadCell()}>Series</div>
          <div style={{ ...tHeadCell(), textAlign: "right" }}>Last</div>
          <div style={{ ...tHeadCell(), textAlign: "right" }}>Δ wk</div>
          <div style={{ ...tHeadCell(), textAlign: "right" }}>Δ ytd</div>
          {WATCHLIST.map((r, i) => {
            const up = r.wk.startsWith("+");
            const flat = r.wk === "0.00%" || r.wk === "+0.00";
            const ch = flat ? "·" : (up ? "▴" : "▾");
            return (
              <React.Fragment key={r.name}>
                <div style={tCell(i === WATCHLIST.length - 1)}>{r.name}</div>
                <div style={{ ...tCell(i === WATCHLIST.length - 1, true), textAlign: "right" }}>{r.last}</div>
                <div style={{
                  ...tCell(i === WATCHLIST.length - 1, true),
                  textAlign: "right",
                  display: "flex", justifyContent: "flex-end", gap: 5,
                }}>
                  <span style={{ color: "var(--ink-2)" }}>{ch}</span>
                  <span>{r.wk.replace("−", "").replace("+", "")}</span>
                </div>
                <div style={{ ...tCell(i === WATCHLIST.length - 1, true), textAlign: "right", color: "var(--ink-2)" }}>{r.ytd}</div>
              </React.Fragment>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function tHeadCell() {
  return {
    fontFamily: "var(--font-sans)",
    fontSize: 10, letterSpacing: "0.08em", textTransform: "uppercase",
    color: "var(--ink-3)",
    padding: "8px 6px",
    borderBottom: "1px solid var(--hairline)",
  };
}
function tCell(last, mono) {
  return {
    fontFamily: mono ? "var(--font-mono)" : "var(--font-sans)",
    fontSize: 13,
    fontVariantNumeric: "tabular-nums lining-nums",
    color: "var(--ink)",
    padding: "8px 6px",
    borderBottom: last ? "none" : "1px solid var(--hairline-soft)",
  };
}

function Retrospective() {
  return (
    <aside style={{
      margin: "32px 0",
      borderTop: "1px solid var(--ink)",
      borderBottom: "1px solid var(--hairline)",
      paddingTop: 12, paddingBottom: 16,
    }}>
      <div style={{
        fontFamily: "var(--font-sans)",
        fontSize: 10, letterSpacing: "0.08em", textTransform: "uppercase",
        color: "var(--ink-3)", marginBottom: 8,
      }}>Retrospective · graded from issue 140</div>
      <div style={{
        fontFamily: "var(--font-serif)",
        fontSize: 17, lineHeight: 1.55, letterSpacing: "-0.006em",
        color: "var(--ink)",
      }}>
        <span style={{ fontStyle: "italic", color: "var(--ink-2)" }}>
          "Energy is being structurally re-rated, and the move has further to run."
        </span>
        <span> &mdash; that call looks early.
        The underlying logic still holds, but the timing was wrong.
        WTI is flat on the four weeks since the issue. We continue to hold
        the thesis; we are no longer holding the timing.</span>
      </div>
    </aside>
  );
}

function AnalystVoices() {
  const voices = [
    { name: "Bull",     stance: "long energy here",              text: "Capex discipline is binding; the marginal barrel is no longer being underwritten. The re-rating completes inside twelve months." },
    { name: "Bear",     stance: "premature",                     text: "Demand destruction is doing more work than the consensus expects. The 2014 analogue is more relevant than the 2007 one." },
    { name: "Balanced", stance: "right thesis, wrong window",    text: "Hold the thesis at the issue level; do not hold the timing. Conditions for revision are below." },
  ];
  return (
    <section style={{
      margin: "32px 0",
      border: "1px solid var(--hairline)", borderRadius: 2,
    }}>
      <div style={{
        padding: "10px 14px",
        borderBottom: "1px solid var(--hairline-soft)",
        fontFamily: "var(--font-sans)",
        fontSize: 10, letterSpacing: "0.08em", textTransform: "uppercase",
        color: "var(--ink-3)",
      }}>Internal stress-test · three voices</div>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr" }}>
        {voices.map((v, i) => (
          <div key={v.name} style={{
            padding: "14px 16px",
            borderRight: i < 2 ? "1px solid var(--hairline-soft)" : "none",
          }}>
            <div style={{
              fontFamily: "var(--font-sans)",
              fontSize: 11, fontWeight: 600, color: "var(--ink)",
              letterSpacing: 0,
            }}>{v.name}</div>
            <div style={{
              fontFamily: "var(--font-sans)",
              fontSize: 10, letterSpacing: "0.05em", textTransform: "uppercase",
              color: "var(--ink-3)", marginTop: 2, marginBottom: 8,
            }}>{v.stance}</div>
            <div style={{
              fontFamily: "var(--font-serif)",
              fontSize: 14, lineHeight: 1.5, letterSpacing: "-0.006em",
              color: "var(--ink-2)",
            }}>{v.text}</div>
          </div>
        ))}
      </div>
    </section>
  );
}

function LatestReport({ report }) {
  return (
    <div style={{ flex: 1, overflowY: "auto", background: "var(--paper)" }}>
      <ReportToolbar />
      <article style={{
        maxWidth: 720,
        margin: "0 auto",
        padding: "56px 32px 96px 32px",
      }}>
        <header style={{ marginBottom: 32 }}>
          <div style={{
            fontFamily: "var(--font-sans)",
            fontSize: 10, letterSpacing: "0.08em", textTransform: "uppercase",
            color: "var(--ink-3)",
            marginBottom: 14,
          }}>Issue {report.id} · weekly</div>
          <h1 style={{
            fontFamily: "var(--font-serif)",
            fontSize: 32, lineHeight: 1.18, fontWeight: 600,
            letterSpacing: 0, color: "var(--ink)",
            margin: "0 0 6px 0",
          }}>{report.title}</h1>
          <div style={{
            fontFamily: "var(--font-serif)",
            fontStyle: "italic", fontSize: 15, color: "var(--ink-3)",
          }}>{report.date} · 9 minutes</div>
        </header>

        <div style={{
          fontFamily: "var(--font-serif)",
          fontSize: 17, lineHeight: 1.55, letterSpacing: "-0.006em",
          color: "var(--ink)",
        }}>
          <p style={{ margin: "0 0 16px 0" }}>
            The thesis is unchanged this week. We are not raising the energy
            call, we are not lowering it, and we are not adding a new one.
            Equity markets drifted; rates drifted; the dollar drifted. There
            is no story to write that did not already exist seven days ago.
          </p>
          <p style={{ margin: "0 0 16px 0" }}>
            That is, by itself, worth noting. The first quarter delivered
            three regime-adjacent moves &mdash; the late-January reversal in
            rate-cut expectations, the early-March repricing of energy, and
            the quiet revision of the soft-landing consensus. None of them
            extended this week. We are watching for the conditions under
            which they would extend; below.
          </p>

          <SectionRule />

          <h2 style={{
            fontFamily: "var(--font-serif)",
            fontSize: 22, lineHeight: 1.3, fontWeight: 600,
            color: "var(--ink)", margin: "0 0 12px 0",
          }}>§1 — State of play</h2>
          <p style={{ margin: "0 0 16px 0" }}>
            Front-end yields are anchored around 4.69%; the back end, around
            4.31%. The curve is flatter than at any point since November,
            and the flattening has come almost entirely from the long end.
            We continue to read this as the bond market grading the
            Committee's projected path against the prints, rather than as
            a fundamental shift in real-rate expectations.
          </p>

          <Figure caption="Figure 1 · 2-yr and 10-yr Treasury yields" source="FRED">
            <YieldChart />
          </Figure>

          <p style={{ margin: "0 0 16px 0" }}>
            We pay attention to the 10-year (in oxblood, above) because that
            is where the re-rating, if it happens, will show up first.
          </p>

          <Watchlist />

          <SectionRule />

          <h2 style={{
            fontFamily: "var(--font-serif)",
            fontSize: 22, lineHeight: 1.3, fontWeight: 600,
            color: "var(--ink)", margin: "0 0 12px 0",
          }}>§2 — Last month's energy call, graded</h2>
          <Retrospective />
          <p style={{ margin: "0 0 16px 0" }}>
            We pulled the trigger early. The structural argument &mdash;
            capex discipline, the unwillingness of the marginal producer to
            underwrite the marginal barrel &mdash; is still the right
            argument. The four-week tape is not validating it. We are not
            changing the issue-level thesis; we are flagging that issue 140
            should not be read as a tactical recommendation.
          </p>

          <SectionRule />

          <h2 style={{
            fontFamily: "var(--font-serif)",
            fontSize: 22, lineHeight: 1.3, fontWeight: 600,
            color: "var(--ink)", margin: "0 0 12px 0",
          }}>§3 — Stress-test of the open thesis</h2>
          <AnalystVoices />
          <p style={{ margin: "0 0 16px 0" }}>
            The Balanced read is the one we will continue to publish under
            our own byline. The Bull case is interesting and is on the
            record above; the Bear case is the one we cannot fully refute,
            and so it sits next to the thesis as a permanent caveat.
          </p>

          <SectionRule />

          <h2 style={{
            fontFamily: "var(--font-serif)",
            fontSize: 22, lineHeight: 1.3, fontWeight: 600,
            color: "var(--ink)", margin: "0 0 12px 0",
          }}>§4 — What would force a revision</h2>
          <p style={{ margin: "0 0 16px 0" }}>
            Two things, named in advance so they are not retrofitted:
          </p>
          <ul style={{ margin: "0 0 16px 0", paddingLeft: 20, listStyle: "square" }}>
            <li style={{ marginBottom: 8 }}>A sustained breach of <span style={{ fontFamily: "var(--font-mono)", fontVariantNumeric: "tabular-nums" }}>$74</span> on the front-month crude contract, held for two consecutive weekly closes.</li>
            <li>A clear inflection in core services inflation in either direction &mdash; specifically, a three-month annualized print outside the 3.4–4.2% corridor.</li>
          </ul>
          <p style={{ margin: "0 0 16px 0" }}>
            Neither has happened. The thesis is unchanged.
          </p>

          <SectionRule />

          <p style={{
            margin: "32px 0 0 0", textAlign: "left",
            fontFamily: "var(--font-serif)",
            fontStyle: "italic", color: "var(--ink-3)", fontSize: 14,
          }}>
            &mdash; Market Signal &middot; Sunday, March 31
          </p>
        </div>
      </article>
    </div>
  );
}

Object.assign(window, { LatestReport, SectionRule, Figure, YieldChart, Watchlist, Retrospective, AnalystVoices, ReportToolbar });
