// WarningBar.jsx — persistent warning area. Always visible. No icon, no
// color flag, no chrome. The words are the alert.

function WarningBar({ children }) {
  if (!children) return null;
  return (
    <div style={{
      display: "flex", alignItems: "baseline", gap: 14,
      padding: "10px 32px",
      borderBottom: "1px solid var(--hairline)",
      background: "var(--paper)",
    }}>
      <div style={{
        fontFamily: "var(--font-sans)",
        fontSize: 10, letterSpacing: "0.08em", textTransform: "uppercase",
        color: "var(--ink)", fontWeight: 600,
        whiteSpace: "nowrap",
      }}>Active warning</div>
      <div style={{
        fontFamily: "var(--font-serif)",
        fontSize: 14, lineHeight: 1.5, letterSpacing: "-0.006em",
        color: "var(--ink)", fontStyle: "italic",
      }}>{children}</div>
    </div>
  );
}

Object.assign(window, { WarningBar });
