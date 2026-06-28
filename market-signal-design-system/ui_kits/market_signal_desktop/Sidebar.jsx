// Sidebar.jsx — the ONE shared-history sidebar. Same structure and treatment
// everywhere; only the content swaps per feature: recent report issues /
// recent Portfolio runs / recent Trade Opportunities runs. Same density, same
// selected-item accent (the oxblood leading-edge rule). A scoped extension of
// the report-history sidebar — not a new navigation pattern for the new pages.

function SidebarHeader({ children }) {
  return (
    <div style={{
      fontFamily: "var(--font-sans)",
      fontSize: 10, letterSpacing: "0.08em", textTransform: "uppercase",
      color: "var(--ink-3)",
      padding: "14px 20px 8px 20px",
    }}>{children}</div>
  );
}

function ReportRow({ report, isCurrent, isNew, onClick }) {
  const [hover, setHover] = React.useState(false);
  return (
    <div
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: "relative", display: "grid", gridTemplateColumns: "1fr max-content",
        gap: 8, alignItems: "baseline", padding: "8px 16px 8px 18px",
        borderLeft: "2px solid " + (isCurrent ? "var(--accent)" : "transparent"),
        borderBottom: "1px solid var(--hairline-soft)",
        background: (isCurrent || hover) ? "var(--paper-soft)" : "transparent",
        cursor: "pointer", transition: "background-color 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
      }}>
      <div style={{ minWidth: 0 }}>
        <div style={{ fontFamily: "var(--font-sans)", fontSize: 13, fontWeight: isCurrent ? 600 : 500, color: "var(--ink)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{report.title}</div>
        <div style={{ fontFamily: "var(--font-sans)", fontSize: 10, letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--ink-3)", marginTop: 2 }}>
          {report.date} · #{report.id}
          {isNew ? <span style={{ marginLeft: 6, color: "var(--accent)" }}>new</span> : null}
        </div>
      </div>
      <div style={{ fontFamily: "var(--font-mono)", fontVariantNumeric: "tabular-nums", fontSize: 11, color: "var(--ink-3)" }}>{report.read}</div>
    </div>
  );
}

// Run row — same density/treatment, content swapped for Portfolio / TO runs.
function RunRow({ run, isCurrent, onClick }) {
  const [hover, setHover] = React.useState(false);
  return (
    <div
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: "relative", display: "grid", gridTemplateColumns: "1fr max-content",
        gap: 8, alignItems: "baseline", padding: "8px 16px 8px 18px",
        borderLeft: "2px solid " + (isCurrent ? "var(--accent)" : "transparent"),
        borderBottom: "1px solid var(--hairline-soft)",
        background: (isCurrent || hover) ? "var(--paper-soft)" : "transparent",
        cursor: "pointer", transition: "background-color 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
      }}>
      <div style={{ minWidth: 0 }}>
        <div style={{ fontFamily: "var(--font-sans)", fontSize: 13, fontWeight: isCurrent ? 600 : 500, color: "var(--ink)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{run.label}</div>
        <div style={{ fontFamily: "var(--font-mono)", fontSize: 10, letterSpacing: "0.04em", color: "var(--ink-3)", marginTop: 2, fontVariantNumeric: "tabular-nums" }}>{run.date}</div>
      </div>
      <div style={{ fontFamily: "var(--font-mono)", fontVariantNumeric: "tabular-nums", fontSize: 11, color: "var(--ink-3)" }}>{run.read}</div>
    </div>
  );
}

function NavItem({ icon, label, badge, active, onClick }) {
  const [hover, setHover] = React.useState(false);
  return (
    <div
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: "flex", alignItems: "center", gap: 10, padding: "8px 18px",
        borderLeft: "2px solid " + (active ? "var(--accent)" : "transparent"),
        background: (active || hover) ? "var(--paper-soft)" : "transparent",
        cursor: "pointer", transition: "background-color 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
        fontFamily: "var(--font-sans)", fontSize: 13, color: "var(--ink)", fontWeight: active ? 600 : 500,
      }}>
      <Icon name={icon} size={14} color="var(--ink-2)" />
      <span style={{ flex: 1 }}>{label}</span>
      {badge != null && (
        <span style={{ fontFamily: "var(--font-mono)", fontVariantNumeric: "tabular-nums", fontSize: 11, color: "var(--ink-3)" }}>{badge}</span>
      )}
    </div>
  );
}

// Maps a view to its owning feature (drives which history list shows).
function featureOf(view) {
  if (view === "portfolio") return "portfolio";
  if (view === "trade") return "trade";
  if (view === "report" || view === "runtracker") return "report";
  return "report"; // inbox / archive / settings keep the report list visible
}

function HistoryList({ feature, currentReportId, setCurrentReportId, currentRunId, setCurrentRunId, setView }) {
  const { RECENT_REPORTS, PORTFOLIO_RUNS, TO_RUNS } = window.MS_DATA;
  if (feature === "portfolio") {
    return (
      <>
        <SidebarHeader>Portfolio runs · recent</SidebarHeader>
        <div style={{ flex: 1, overflowY: "auto", borderTop: "1px solid var(--hairline)" }}>
          {PORTFOLIO_RUNS.map(r => (
            <RunRow key={r.id} run={r} isCurrent={r.id === currentRunId} onClick={() => { setView("portfolio"); setCurrentRunId(r.id); }} />
          ))}
        </div>
      </>
    );
  }
  if (feature === "trade") {
    return (
      <>
        <SidebarHeader>Trade Opportunities runs · recent</SidebarHeader>
        <div style={{ flex: 1, overflowY: "auto", borderTop: "1px solid var(--hairline)" }}>
          {TO_RUNS.map(r => (
            <RunRow key={r.id} run={r} isCurrent={r.id === currentRunId} onClick={() => { setView("trade"); setCurrentRunId(r.id); }} />
          ))}
        </div>
      </>
    );
  }
  return (
    <>
      <SidebarHeader>Recent reports · last 30</SidebarHeader>
      <div style={{ flex: 1, overflowY: "auto", borderTop: "1px solid var(--hairline)" }}>
        {RECENT_REPORTS.map(r => (
          <ReportRow key={r.id} report={r} isNew={r.isNew} isCurrent={r.id === currentReportId} onClick={() => { setView("report"); setCurrentReportId(r.id); }} />
        ))}
      </div>
    </>
  );
}

function Sidebar({ view, setView, currentReportId, setCurrentReportId, currentRunId, setCurrentRunId, feature: featureProp }) {
  const feature = featureProp || featureOf(view);
  return (
    <aside style={{ width: 280, flexShrink: 0, borderRight: "1px solid var(--hairline)", background: "var(--paper)", display: "flex", flexDirection: "column", minHeight: 0 }}>
      {/* Feature nav — three primary surfaces, same row treatment */}
      <div style={{ borderBottom: "1px solid var(--hairline)", paddingBottom: 4 }}>
        <SidebarHeader>Market Signal</SidebarHeader>
        <NavItem icon="report"   label="Weekly report"      active={feature === "report"}    onClick={() => setView("report")} />
        <NavItem icon="rule"     label="Portfolio analysis" active={feature === "portfolio"} onClick={() => setView("portfolio")} />
        <NavItem icon="search"   label="Trade opportunities" active={feature === "trade"}    onClick={() => setView("trade")} />
      </div>

      {/* Shared history list — content swaps per feature */}
      <HistoryList
        feature={feature}
        currentReportId={currentReportId} setCurrentReportId={setCurrentReportId}
        currentRunId={currentRunId} setCurrentRunId={setCurrentRunId}
        setView={setView}
      />

      {/* Chrome nav */}
      <div style={{ borderTop: "1px solid var(--hairline)", paddingTop: 4 }}>
        <NavItem icon="inbox" label="Research inbox" badge="7" active={view === "inbox"} onClick={() => setView("inbox")} />
        <NavItem icon="archive" label="Archive" active={view === "archive"} onClick={() => setView("archive")} />
        <NavItem icon="settings" label="Settings" active={view === "settings"} onClick={() => setView("settings")} />
      </div>
    </aside>
  );
}

Object.assign(window, { Sidebar, ReportRow, RunRow, NavItem, SidebarHeader, HistoryList, featureOf });
