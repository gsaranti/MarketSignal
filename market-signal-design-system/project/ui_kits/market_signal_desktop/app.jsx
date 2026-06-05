// app.jsx — top-level state + view router.
//
// ⚠ SUPERSEDED (2026-06-04 UX pass): the footer/status row is run-status only —
// the weekly on/off toggle lives in Settings, and the footer no longer shows the
// (always-truncated) failure reason, just the timestamp; the full reason is in
// the warning band. Footer + sidebar sit on --paper-soft. See project/README.md.

function StatusRow({ generating, onGenerate }) {
  if (!generating) {
    return (
      <div style={{
        display: "flex", alignItems: "center", justifyContent: "space-between",
        padding: "8px 32px",
        borderTop: "1px solid var(--hairline)",
        background: "var(--paper)",
      }}>
        <div style={{
          fontFamily: "var(--font-sans)",
          fontSize: 11, letterSpacing: "0.05em", textTransform: "uppercase",
          color: "var(--ink-3)",
          whiteSpace: "nowrap",
        }}>Idle · next scheduled run · Sun 04:00 ET</div>
        <button
          onClick={onGenerate}
          style={{
            display: "inline-flex", alignItems: "center", gap: 6,
            padding: "5px 10px",
            fontFamily: "var(--font-sans)",
            fontSize: 12, fontWeight: 500,
            whiteSpace: "nowrap",
            border: "1px solid var(--ink)", borderRadius: 2,
            background: "transparent", color: "var(--ink)",
            cursor: "pointer",
            transition: "background 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
          }}>Generate now</button>
      </div>
    );
  }
  return (
    <div style={{
      display: "flex", alignItems: "center", gap: 14,
      padding: "8px 32px",
      borderTop: "1px solid var(--hairline)",
      background: "var(--paper)",
    }}>
      <div style={{
        fontFamily: "var(--font-sans)",
        fontSize: 12, color: "var(--ink-2)",
        whiteSpace: "nowrap",
      }}>Generating this week's issue</div>
      <div style={{
        flex: 1, height: 1, background: "var(--hairline-soft)",
        position: "relative", overflow: "hidden",
      }}>
        <div style={{
          position: "absolute", left: 0, top: 0, bottom: 0,
          width: "38%", background: "var(--ink)",
        }} />
      </div>
      <div style={{
        fontFamily: "var(--font-mono)",
        fontVariantNumeric: "tabular-nums",
        fontSize: 11, color: "var(--ink-3)",
        whiteSpace: "nowrap",
      }}>step 4 of 11 · ~24 min</div>
    </div>
  );
}

function App() {
  const { RECENT_REPORTS } = window.MS_DATA;
  const [view, setView] = React.useState("report");                    // 'report' | 'inbox' | 'archive' | 'settings'
  const [currentReportId, setCurrentReportId] = React.useState(142);
  const [generating, setGenerating] = React.useState(false);

  const report = RECENT_REPORTS.find(r => r.id === currentReportId) || RECENT_REPORTS[0];

  // Archive routes to inbox for now (the brief pairs them).
  const effectiveView = view === "archive" ? "inbox" : view;

  return (
    <MarketSignalWindow width={1280} height={820}>
      <Sidebar
        view={effectiveView}
        setView={setView}
        currentReportId={currentReportId}
        setCurrentReportId={setCurrentReportId}
      />
      <main style={{
        flex: 1, display: "flex", flexDirection: "column",
        minWidth: 0, background: "var(--paper)",
      }}>
        <WarningBar>
          Last month's energy call (issue 140) was early. The underlying logic still holds; the timing was wrong. See the retrospective in §2.
        </WarningBar>

        <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
          {effectiveView === "report"   && <LatestReport report={report} />}
          {effectiveView === "inbox"    && <ResearchInbox />}
          {effectiveView === "settings" && <Settings />}
        </div>

        <StatusRow
          generating={generating}
          onGenerate={() => setGenerating(true)}
        />
      </main>
    </MarketSignalWindow>
  );
}

// Mount
const root = ReactDOM.createRoot(document.getElementById("root"));
root.render(<App />);
