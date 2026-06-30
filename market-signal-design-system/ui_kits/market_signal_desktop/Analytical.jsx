// Analytical.jsx — shared primitives for the analytical register
// (Portfolio + Trade Opportunities). Denser than the report; mono numerics
// first-class; the desaturated palette for direction and grades. Still flat
// with hairlines — no shadow, no pill, no radius > 2px, no celebratory framing.

// ---- Tracked-caps label (11px, +0.05em) ----
function AnaHead({ children, color = "var(--ink-3)", style }) {
  return (
    <div style={{
      fontFamily: "var(--font-sans)",
      fontSize: 11, fontWeight: 600, letterSpacing: "0.05em",
      textTransform: "uppercase", color,
      ...style,
    }}>{children}</div>
  );
}

// ---- Directional value token — sign + weight + chevron + desaturated hue ----
const DIR_META = {
  up:   { color: "var(--ana-up)",   ch: "\u25B4" },
  down: { color: "var(--ana-down)", ch: "\u25BE" },
  flat: { color: "var(--ana-flat)", ch: "\u00B7" },
};
function Dir({ dir = "flat", children, size = 13, style }) {
  const m = DIR_META[dir] || DIR_META.flat;
  return (
    <span style={{
      fontFamily: "var(--font-mono)",
      fontVariantNumeric: "tabular-nums lining-nums",
      fontWeight: 500, fontSize: size, letterSpacing: "-0.01em",
      color: m.color, display: "inline-flex", alignItems: "baseline", gap: 4,
      ...style,
    }}>
      <span aria-hidden="true">{m.ch}</span>
      <span>{children}</span>
    </span>
  );
}

// ---- Grade chip — hairline/flat, never glossy ----
const GRADE_KEY = { A: "a", B: "b", C: "c", D: "d", E: "f", F: "f" };
function gradeVars(letter) {
  const k = GRADE_KEY[(letter || "C")[0].toUpperCase()] || "c";
  return { tx: `var(--grade-${k}-tx)`, bg: `var(--grade-${k}-bg)` };
}
function Grade({ value = "C", size = "md", style }) {
  const { tx, bg } = gradeVars(value);
  const dims = size === "lg"
    ? { minWidth: 34, height: 30, fontSize: 18 }
    : size === "sm"
      ? { minWidth: 22, height: 19, fontSize: 12 }
      : { minWidth: 26, height: 22, fontSize: 14 };
  return (
    <span style={{
      display: "inline-flex", alignItems: "center", justifyContent: "center",
      padding: "0 6px", fontFamily: "var(--font-mono)", fontWeight: 600,
      lineHeight: 1, border: "1px solid var(--hairline)", borderRadius: 2,
      color: tx, background: bg, ...dims, ...style,
    }}>{value}</span>
  );
}

// ---- Conviction meter — flat hairline scale ----
function Conviction({ value = 0, of = 5, style }) {
  return (
    <span style={{ display: "inline-flex", gap: 3, alignItems: "center", ...style }}>
      {Array.from({ length: of }).map((_, i) => (
        <span key={i} style={{
          width: 14, height: 6, borderRadius: 1, display: "block",
          border: "1px solid " + (i < value ? "var(--ink-2)" : "var(--hairline)"),
          background: i < value ? "var(--ink-2)" : "transparent",
        }} />
      ))}
    </span>
  );
}

// ---- Key-figure strip — hairline-delimited label-over-value ----
function KeyFigureStrip({ items, style }) {
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

// ---- Restrained sparkline — single ink weight, one accent series ----
function Sparkline({ data = [], dir = "up", w = 120, h = 32, baseline = true }) {
  if (!data.length) return null;
  const min = Math.min(...data), max = Math.max(...data);
  const span = (max - min) || 1;
  const P = 3;
  const x = i => P + i * ((w - 2 * P) / (data.length - 1));
  const y = v => P + (1 - (v - min) / span) * (h - 2 * P);
  const path = data.map((v, i) => (i ? "L" : "M") + x(i).toFixed(1) + " " + y(v).toFixed(1)).join(" ");
  const stroke = (DIR_META[dir] || DIR_META.up).color;
  return (
    <svg viewBox={`0 0 ${w} ${h}`} width={w} height={h} preserveAspectRatio="none" style={{ flexShrink: 0, display: "block" }}>
      {baseline && <line x1="0" y1={h - P} x2={w} y2={h - P} stroke="var(--hairline-soft)" strokeWidth="0.5" />}
      <path d={path} fill="none" stroke={stroke} strokeWidth="1.25" />
    </svg>
  );
}

// ---- Methodology affordance — restrained reveal ("how this was computed") ----
function Methodology({ note, label = "how" }) {
  const [open, setOpen] = React.useState(false);
  return (
    <span style={{ position: "relative", display: "inline-block" }}>
      <span
        onClick={() => setOpen(o => !o)}
        style={{
          fontFamily: "var(--font-sans)", fontSize: 11, letterSpacing: "0.02em",
          color: "var(--ink-3)", borderBottom: "1px dotted var(--hairline)",
          cursor: "help", userSelect: "none",
        }}>{label}</span>
      {open && (
        <span style={{
          position: "absolute", left: 0, top: "calc(100% + 6px)", zIndex: 10,
          width: 240, padding: "10px 12px",
          background: "var(--paper-edge)", border: "1px solid var(--hairline)",
          borderRadius: 2,
          fontFamily: "var(--font-serif)", fontSize: 12, lineHeight: 1.5,
          letterSpacing: "-0.006em", color: "var(--ink-2)",
        }}>{note}</span>
      )}
    </span>
  );
}

// ---- Card wrapper — flat hairline rectangle ----
function AnaCard({ children, style }) {
  return (
    <div style={{
      background: "var(--paper)", border: "1px solid var(--hairline)",
      borderRadius: 2, ...style,
    }}>{children}</div>
  );
}

// ---- Reveal disclosure — for falsifiers / triggers / lineage (density control) ----
function Reveal({ label, children }) {
  const [open, setOpen] = React.useState(false);
  return (
    <div>
      <button
        onClick={() => setOpen(o => !o)}
        style={{
          display: "inline-flex", alignItems: "center", gap: 6,
          background: "transparent", border: 0, padding: "2px 0", cursor: "pointer",
          fontFamily: "var(--font-sans)", fontSize: 11, fontWeight: 600,
          letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--ink-3)",
        }}>
        <span style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>{open ? "\u25BE" : "\u25B8"}</span>
        {label}
      </button>
      {open && <div style={{ marginTop: 8 }}>{children}</div>}
    </div>
  );
}

// ---- Sort bar — card-surface control for a holdings card stack (Portfolio).
// A row of toggle TRIGGERS, one per sort key — NOT a table, so each is a
// <button aria-pressed> and NEVER carries aria-sort (reserved for .ana-grid
// heads). The active key shows the EXACT ▾/▴ glyph the grid heads and Dir use;
// inactive keys a dimmed ▾ at th.sortable's 0.5 opacity. Clicking the active
// key flips direction. Controlled (value) or uncontrolled (defaultValue).
// `keys`: [{ key, label }]. Sort shape: { key, dir: "asc" | "desc" }.
function SortBar({ keys = [], value, defaultValue, onChange, label = "Sort", style }) {
  const norm = v => (typeof v === "string" ? { key: v, dir: "desc" } : v);
  const [internal, setInternal] = React.useState(
    () => norm(defaultValue) || { key: keys[0] && keys[0].key, dir: "desc" }
  );
  const sort = value != null ? norm(value) : internal;
  const pick = k => {
    const next = sort.key === k
      ? { key: k, dir: sort.dir === "desc" ? "asc" : "desc" }   // flip the active key
      : { key: k, dir: "desc" };                                // new key opens descending
    if (value == null) setInternal(next);
    onChange && onChange(next);
  };
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 4, flexWrap: "wrap", ...style }}>
      {label && (
        <span style={{
          fontFamily: "var(--font-sans)", fontSize: 11, fontWeight: 600,
          letterSpacing: "0.05em", textTransform: "uppercase", color: "var(--ink-3)",
          marginRight: 8,
        }}>{label}</span>
      )}
      {keys.map(k => {
        const active = sort.key === k.key;
        return (
          <button key={k.key} type="button" aria-pressed={active}
            aria-label={`Sort by ${k.label}${active ? `, ${sort.dir === "asc" ? "ascending" : "descending"}` : ""}`}
            onClick={() => pick(k.key)}
            style={{
              display: "inline-flex", alignItems: "baseline", gap: 5,
              fontFamily: "var(--font-sans)", fontSize: 11, fontWeight: 600,
              letterSpacing: "0.05em", textTransform: "uppercase",
              color: active ? "var(--ink)" : "var(--ink-3)",
              background: active ? "var(--paper-soft)" : "transparent",
              border: "1px solid " + (active ? "var(--hairline)" : "transparent"),
              borderRadius: 2, padding: "5px 8px", cursor: "pointer",
              transition: "all 120ms cubic-bezier(0.4,0.0,0.2,1)",
            }}>
            <span>{k.label}</span>
            <span aria-hidden="true" style={{
              fontFamily: "var(--font-mono)", letterSpacing: 0, opacity: active ? 1 : 0.5,
            }}>{active ? (sort.dir === "asc" ? "▴" : "▾") : "▾"}</span>
          </button>
        );
      })}
    </div>
  );
}

// ---- Matrix / list view toggle — Trade Opportunities. A minimal two-option
// switch (Matrix · List). Ghost-text on the .btn-ghost posture, tracked-caps
// options, active marked via aria-pressed. Segmented WITHOUT a capsule:
// hairline-divided, radius ≤ 2px — never a pill, never a tab bar.
function ViewToggle({ options, value, defaultValue, onChange, label = "View", style }) {
  const opts = options || [
    { key: "matrix", label: "Matrix" },
    { key: "list",   label: "List" },
  ];
  const [internal, setInternal] = React.useState(defaultValue || opts[0].key);
  const cur = value != null ? value : internal;
  const pick = k => { if (value == null) setInternal(k); onChange && onChange(k); };
  return (
    <div role="group" aria-label={label} style={{
      display: "inline-flex", border: "1px solid var(--hairline)",
      borderRadius: 2, overflow: "hidden", ...style,
    }}>
      {opts.map((o, i) => {
        const active = cur === o.key;
        return (
          <button key={o.key} type="button" aria-pressed={active} onClick={() => pick(o.key)}
            style={{
              fontFamily: "var(--font-sans)", fontSize: 11, fontWeight: 600,
              letterSpacing: "0.05em", textTransform: "uppercase",
              color: active ? "var(--ink)" : "var(--ink-3)",
              background: active ? "var(--paper-soft)" : "transparent",
              border: 0, borderLeft: i === 0 ? "0" : "1px solid var(--hairline)",
              padding: "6px 12px", cursor: "pointer",
              transition: "all 120ms cubic-bezier(0.4,0.0,0.2,1)",
            }}>
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

Object.assign(window, {
  AnaHead, Dir, Grade, Conviction, KeyFigureStrip, Sparkline, Methodology, AnaCard, Reveal,
  SortBar, ViewToggle,
});
