// app.jsx — top-level state + view router. Adds the analytical surfaces
// (Portfolio, Trade Opportunities) and the one shared, leaveable run tracker.

// ---- Job-status footer — the run lives here. Not a modal. ----
function JobFooter({ job, feature, onStart, onView, onDismiss }) {
  const wrap = {
    display: "flex", alignItems: "center", gap: 14,
    padding: "8px 32px", borderTop: "1px solid var(--hairline)", background: "var(--paper)",
  };
  const startLabel = { report: "Generate now", portfolio: "Run analysis", trade: "Run discovery" }[feature] || "Run";

  if (job.state === "running") {
    return (
      <div style={wrap}>
        <div style={{ fontFamily: "var(--font-sans)", fontSize: 12, color: "var(--ink-2)", whiteSpace: "nowrap" }}>
          {JOB_TITLE[job.kind]} · running in background
        </div>
        <div style={{ flex: 1, height: 1, background: "var(--hairline-soft)", position: "relative", overflow: "hidden" }}>
          <div style={{ position: "absolute", left: 0, top: 0, bottom: 0, width: "46%", background: "var(--ink)" }} />
        </div>
        <button onClick={onView} style={footerBtn(true)}>View progress</button>
      </div>
    );
  }
  if (job.state === "done") {
    return (
      <div style={{ ...wrap, justifyContent: "space-between" }}>
        <div style={{ fontFamily: "var(--font-sans)", fontSize: 11, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--ink-3)", whiteSpace: "nowrap" }}>
          {JOB_TITLE[job.kind]} · complete · trace kept for this session
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          <button onClick={onView} style={footerBtn(false)}>Latest run log</button>
          <button onClick={onStart} style={footerBtn(true)}>{startLabel}</button>
        </div>
      </div>
    );
  }
  // idle
  return (
    <div style={{ ...wrap, justifyContent: "space-between" }}>
      <div style={{ fontFamily: "var(--font-sans)", fontSize: 11, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--ink-3)", whiteSpace: "nowrap" }}>
        Idle · only one run at a time · last completed Apr 12
      </div>
      <button onClick={onStart} style={footerBtn(true)}>{startLabel}</button>
    </div>
  );
}
const JOB_TITLE = { report: "Weekly issue", portfolio: "Portfolio analysis", trade: "Trade discovery" };
function footerBtn(primary) {
  return {
    display: "inline-flex", alignItems: "center", gap: 6, padding: "5px 11px",
    fontFamily: "var(--font-sans)", fontSize: 12, fontWeight: 500, whiteSpace: "nowrap",
    border: "1px solid " + (primary ? "var(--ink)" : "var(--hairline)"),
    background: primary ? "var(--ink)" : "transparent",
    color: primary ? "var(--paper)" : "var(--ink-2)", cursor: "pointer", borderRadius: 2,
    transition: "all 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
  };
}

// ---- Per-view warning content (same band treatment everywhere) ----
const WARNINGS = {
  report: { tag: "Active warning", text: "Last month's energy call (issue 140) was early. The underlying logic still holds; the timing was wrong. See the retrospective in §2." },
  portfolio: { tag: "Schwab · re-auth", text: "Your Schwab access token expires in 3 days. Re-authenticate before the next run — the analysis job is gated on a live connection." },
  trade: { tag: "Calibration", text: "Trade Opportunities is in shadow mode. The calibration scorecard is shown honestly but is not yet steering which ideas surface." },
};

function App() {
  const { RECENT_REPORTS } = window.MS_DATA;
  const [view, setView] = React.useState("portfolio");
  const [currentReportId, setCurrentReportId] = React.useState(142);
  const [currentRunId, setCurrentRunId] = React.useState("pf-0412");
  // one run at a time across the whole app
  const [job, setJob] = React.useState({ kind: "report", state: "idle", returnTo: "report" });

  const report = RECENT_REPORTS.find(r => r.id === currentReportId) || RECENT_REPORTS[0];
  const effectiveView = view === "archive" ? "inbox" : view;
  const feature = window.featureOf(view === "runtracker" ? job.returnTo : view);
  const warning = WARNINGS[feature] || WARNINGS.report;

  const startRun = () => {
    const f = feature;
    setJob({ kind: f, state: "running", returnTo: f === "report" ? "report" : f });
    setView("runtracker");
  };
  const leaveTracker = () => setView(job.returnTo);
  const cancelRun = () => { setJob(j => ({ ...j, state: "idle" })); setView(job.returnTo); };
  const viewProgress = () => setView("runtracker");

  return (
    <MarketSignalWindow width={1280} height={840}>
      <Sidebar
        view={view} setView={setView}
        feature={feature}
        currentReportId={currentReportId} setCurrentReportId={setCurrentReportId}
        currentRunId={currentRunId} setCurrentRunId={setCurrentRunId}
      />
      <main style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0, background: "var(--paper)" }}>
        <WarningBar tag={warning.tag}>{warning.text}</WarningBar>

        <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
          {view === "runtracker"        && <RunTracker kind={job.kind} onLeave={leaveTracker} onCancel={cancelRun} />}
          {view !== "runtracker" && effectiveView === "report"    && <LatestReport report={report} />}
          {view !== "runtracker" && effectiveView === "portfolio" && <Portfolio />}
          {view !== "runtracker" && effectiveView === "trade"     && <TradeOpportunities />}
          {view !== "runtracker" && effectiveView === "inbox"     && <ResearchInbox />}
          {view !== "runtracker" && effectiveView === "settings"  && <Settings />}
        </div>

        <JobFooter
          job={job} feature={feature}
          onStart={startRun} onView={viewProgress}
          onDismiss={() => setJob(j => ({ ...j, state: "idle" }))}
        />
      </main>
    </MarketSignalWindow>
  );
}

const root = ReactDOM.createRoot(document.getElementById("root"));
root.render(<App />);
