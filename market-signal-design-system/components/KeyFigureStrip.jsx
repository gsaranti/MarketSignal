// KeyFigureStrip — the analytical register's at-a-glance scan unit. A flat,
// hairline-delimited row of label-over-value pairs; values in mono tabular.
// No fill, no shadow, no pill.

export function KeyFigureStrip({ items = [], style }) {
  return (
    <div style={{
      display: "grid", gridAutoFlow: "column", gridAutoColumns: "1fr",
      border: "1px solid var(--hairline)", borderRadius: 2,
      background: "var(--paper)", ...style,
    }}>
      {items.map((it, i) => (
        <div key={i} style={{
          padding: "10px 14px",
          borderLeft: i === 0 ? "0" : "1px solid var(--hairline-soft)",
        }}>
          <div style={{
            fontFamily: "var(--font-sans)", fontSize: 10, letterSpacing: "0.05em",
            textTransform: "uppercase", color: "var(--ink-3)", marginBottom: 5,
            whiteSpace: "nowrap",
          }}>{it.label}</div>
          <div style={{
            fontFamily: it.sans ? "var(--font-sans)" : "var(--font-mono)",
            fontVariantNumeric: "tabular-nums lining-nums",
            fontSize: it.sans ? 14 : 16, color: "var(--ink)", letterSpacing: "-0.01em",
          }}>{it.value}</div>
        </div>
      ))}
    </div>
  );
}
