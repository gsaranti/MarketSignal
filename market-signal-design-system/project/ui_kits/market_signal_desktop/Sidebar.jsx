// Sidebar.jsx — Recent Reports list (dense, hairline-ruled rows) plus
// nav targets at the foot of the panel: Inbox, Archive, Settings.

function SidebarHeader({ children }) {
  return (
    <div style={{
      fontFamily: "var(--font-sans)",
      fontSize: 10, letterSpacing: "0.08em", textTransform: "uppercase",
      color: "var(--ink-3)",
      padding: "16px 20px 8px 20px",
    }}>{children}</div>
  );
}

function ReportRow({ report, isCurrent, isNew, onClick }) {
  const [hover, setHover] = React.useState(false);
  const bg = isCurrent
    ? "var(--paper-soft)"
    : hover ? "var(--paper-soft)" : "transparent";
  return (
    <div
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: "relative",
        display: "grid",
        gridTemplateColumns: "1fr max-content",
        gap: 8, alignItems: "baseline",
        padding: "8px 16px 8px 18px",
        borderLeft: "2px solid " + (isCurrent ? "var(--accent)" : "transparent"),
        borderBottom: "1px solid var(--hairline-soft)",
        background: bg,
        cursor: "pointer",
        transition: "background-color 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
      }}>
      <div style={{ minWidth: 0 }}>
        <div style={{
          fontFamily: "var(--font-sans)",
          fontSize: 13, fontWeight: isCurrent ? 600 : 500,
          color: "var(--ink)",
          overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
        }}>
          {report.title}
        </div>
        <div style={{
          fontFamily: "var(--font-sans)",
          fontSize: 10, letterSpacing: "0.05em", textTransform: "uppercase",
          color: "var(--ink-3)", marginTop: 2,
        }}>
          {report.date} · #{report.id}
          {isNew ? <span style={{ marginLeft: 6, color: "var(--accent)" }}>new</span> : null}
        </div>
      </div>
      <div style={{
        fontFamily: "var(--font-mono)",
        fontVariantNumeric: "tabular-nums",
        fontSize: 11, color: "var(--ink-3)",
      }}>{report.read}</div>
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
        display: "flex", alignItems: "center", gap: 10,
        padding: "8px 18px",
        borderLeft: "2px solid " + (active ? "var(--accent)" : "transparent"),
        background: active ? "var(--paper-soft)" : (hover ? "var(--paper-soft)" : "transparent"),
        cursor: "pointer",
        transition: "background-color 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
        fontFamily: "var(--font-sans)",
        fontSize: 13, color: "var(--ink)",
        fontWeight: active ? 600 : 500,
      }}>
      <Icon name={icon} size={14} color="var(--ink-2)" />
      <span style={{ flex: 1 }}>{label}</span>
      {badge != null && (
        <span style={{
          fontFamily: "var(--font-mono)",
          fontVariantNumeric: "tabular-nums",
          fontSize: 11, color: "var(--ink-3)",
        }}>{badge}</span>
      )}
    </div>
  );
}

function Sidebar({ view, setView, currentReportId, setCurrentReportId }) {
  const { RECENT_REPORTS } = window.MS_DATA;
  return (
    <aside style={{
      width: 280, flexShrink: 0,
      borderRight: "1px solid var(--hairline)",
      background: "var(--paper)",
      display: "flex", flexDirection: "column",
      minHeight: 0,
    }}>
      {/* Recent Reports header */}
      <SidebarHeader>Recent Reports · last 30</SidebarHeader>
      <div style={{
        flex: 1, overflowY: "auto",
        borderTop: "1px solid var(--hairline)",
      }}>
        {RECENT_REPORTS.map(r => (
          <ReportRow
            key={r.id}
            report={r}
            isNew={r.isNew}
            isCurrent={view === "report" && r.id === currentReportId}
            onClick={() => { setView("report"); setCurrentReportId(r.id); }}
          />
        ))}
      </div>

      {/* Bottom nav */}
      <div style={{ borderTop: "1px solid var(--hairline)", paddingTop: 4 }}>
        <NavItem
          icon="inbox" label="Research Inbox" badge="7"
          active={view === "inbox"}
          onClick={() => setView("inbox")} />
        <NavItem
          icon="archive" label="Archive"
          active={view === "archive"}
          onClick={() => setView("archive")} />
        <NavItem
          icon="settings" label="Settings"
          active={view === "settings"}
          onClick={() => setView("settings")} />
      </div>
    </aside>
  );
}

Object.assign(window, { Sidebar, ReportRow, NavItem, SidebarHeader });
