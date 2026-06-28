/* @ds-bundle: {"format":3,"namespace":"MarketSignalDesignSystem_5eede4","components":[{"name":"DirectionalValue","sourcePath":"components/DirectionalValue.jsx"},{"name":"GradeChip","sourcePath":"components/GradeChip.jsx"},{"name":"KeyFigureStrip","sourcePath":"components/KeyFigureStrip.jsx"}],"sourceHashes":{"components/DirectionalValue.jsx":"5a7d7f403a5c","components/GradeChip.jsx":"524a3594e11a","components/KeyFigureStrip.jsx":"2602df40f4b5","ui_kits/market_signal_desktop/Analytical.jsx":"68ea37e3c1a6","ui_kits/market_signal_desktop/Icon.jsx":"3ef38a718d3b","ui_kits/market_signal_desktop/LatestReport.jsx":"108bdcea7591","ui_kits/market_signal_desktop/Portfolio.jsx":"1125c77de7be","ui_kits/market_signal_desktop/ResearchInbox.jsx":"142f8c55cd60","ui_kits/market_signal_desktop/RunTracker.jsx":"0d84f57f777c","ui_kits/market_signal_desktop/Settings.jsx":"cea5eef0ad47","ui_kits/market_signal_desktop/Sidebar.jsx":"9fed3c0fa926","ui_kits/market_signal_desktop/TradeOpportunities.jsx":"94489a18668c","ui_kits/market_signal_desktop/WarningBar.jsx":"0f69e6a82b65","ui_kits/market_signal_desktop/Window.jsx":"e8c6b67cb6ad","ui_kits/market_signal_desktop/app.jsx":"db394e67c32f","ui_kits/market_signal_desktop/data.js":"9f5baa77d763","ui_kits/market_signal_desktop/data_analytical.js":"873489257fab"},"inlinedExternals":[],"unexposedExports":[]} */

(() => {

const __ds_ns = (window.MarketSignalDesignSystem_5eede4 = window.MarketSignalDesignSystem_5eede4 || {});

const __ds_scope = {};

(__ds_ns.__errors = __ds_ns.__errors || []);

// components/DirectionalValue.jsx
try { (() => {
// DirectionalValue — the up/down/flat treatment for the analytical register.
// Sign + weight + chevron + desaturated hue (muted-green up / oxblood down /
// neutral flat). Still no saturated red/green. Mono tabular figures.

const DIR_META = {
  up: {
    color: "var(--ana-up)",
    ch: "\u25B4"
  },
  down: {
    color: "var(--ana-down)",
    ch: "\u25BE"
  },
  flat: {
    color: "var(--ana-flat)",
    ch: "\u00B7"
  }
};
function DirectionalValue({
  dir = "flat",
  children,
  size = 13,
  style
}) {
  const m = DIR_META[dir] || DIR_META.flat;
  return /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontVariantNumeric: "tabular-nums lining-nums",
      fontWeight: 500,
      fontSize: size,
      letterSpacing: "-0.01em",
      color: m.color,
      display: "inline-flex",
      alignItems: "baseline",
      gap: 4,
      ...style
    }
  }, /*#__PURE__*/React.createElement("span", {
    "aria-hidden": "true"
  }, m.ch), /*#__PURE__*/React.createElement("span", null, children));
}
Object.assign(__ds_scope, { DirectionalValue });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/DirectionalValue.jsx", error: String((e && e.message) || e) }); }

// components/GradeChip.jsx
try { (() => {
// GradeChip — a discrete tonal grade chip (A–F) from the unified analytical
// palette. Hairline/flat, never a glossy badge. Analytical register only.
// Reads --grade-{a..f}-tx / --grade-{a..f}-bg from colors_and_type.css.

const GRADE_KEY = {
  A: "a",
  B: "b",
  C: "c",
  D: "d",
  E: "f",
  F: "f"
};
function GradeChip({
  value = "C",
  size = "md",
  style
}) {
  const k = GRADE_KEY[String(value || "C")[0].toUpperCase()] || "c";
  const dims = size === "lg" ? {
    minWidth: 34,
    height: 30,
    fontSize: 18
  } : size === "sm" ? {
    minWidth: 22,
    height: 19,
    fontSize: 12
  } : {
    minWidth: 26,
    height: 22,
    fontSize: 14
  };
  return /*#__PURE__*/React.createElement("span", {
    style: {
      display: "inline-flex",
      alignItems: "center",
      justifyContent: "center",
      padding: "0 6px",
      fontFamily: "var(--font-mono)",
      fontWeight: 600,
      lineHeight: 1,
      letterSpacing: 0,
      border: "1px solid var(--hairline)",
      borderRadius: 2,
      color: `var(--grade-${k}-tx)`,
      background: `var(--grade-${k}-bg)`,
      ...dims,
      ...style
    }
  }, value);
}
Object.assign(__ds_scope, { GradeChip });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/GradeChip.jsx", error: String((e && e.message) || e) }); }

// components/KeyFigureStrip.jsx
try { (() => {
// KeyFigureStrip — the analytical register's at-a-glance scan unit. A flat,
// hairline-delimited row of label-over-value pairs; values in mono tabular.
// No fill, no shadow, no pill.

function KeyFigureStrip({
  items = [],
  style
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridAutoFlow: "column",
      gridAutoColumns: "1fr",
      border: "1px solid var(--hairline)",
      borderRadius: 2,
      background: "var(--paper)",
      ...style
    }
  }, items.map((it, i) => /*#__PURE__*/React.createElement("div", {
    key: i,
    style: {
      padding: "10px 14px",
      borderLeft: i === 0 ? "0" : "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)",
      marginBottom: 5,
      whiteSpace: "nowrap"
    }
  }, it.label), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: it.sans ? "var(--font-sans)" : "var(--font-mono)",
      fontVariantNumeric: "tabular-nums lining-nums",
      fontSize: it.sans ? 14 : 16,
      color: "var(--ink)",
      letterSpacing: "-0.01em"
    }
  }, it.value))));
}
Object.assign(__ds_scope, { KeyFigureStrip });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/KeyFigureStrip.jsx", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/Analytical.jsx
try { (() => {
// Analytical.jsx — shared primitives for the analytical register
// (Portfolio + Trade Opportunities). Denser than the report; mono numerics
// first-class; the desaturated palette for direction and grades. Still flat
// with hairlines — no shadow, no pill, no radius > 2px, no celebratory framing.

// ---- Tracked-caps label (11px, +0.05em) ----
function AnaHead({
  children,
  color = "var(--ink-3)",
  style
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      fontWeight: 600,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color,
      ...style
    }
  }, children);
}

// ---- Directional value token — sign + weight + chevron + desaturated hue ----
const DIR_META = {
  up: {
    color: "var(--ana-up)",
    ch: "\u25B4"
  },
  down: {
    color: "var(--ana-down)",
    ch: "\u25BE"
  },
  flat: {
    color: "var(--ana-flat)",
    ch: "\u00B7"
  }
};
function Dir({
  dir = "flat",
  children,
  size = 13,
  style
}) {
  const m = DIR_META[dir] || DIR_META.flat;
  return /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontVariantNumeric: "tabular-nums lining-nums",
      fontWeight: 500,
      fontSize: size,
      letterSpacing: "-0.01em",
      color: m.color,
      display: "inline-flex",
      alignItems: "baseline",
      gap: 4,
      ...style
    }
  }, /*#__PURE__*/React.createElement("span", {
    "aria-hidden": "true"
  }, m.ch), /*#__PURE__*/React.createElement("span", null, children));
}

// ---- Grade chip — hairline/flat, never glossy ----
const GRADE_KEY = {
  A: "a",
  B: "b",
  C: "c",
  D: "d",
  E: "f",
  F: "f"
};
function gradeVars(letter) {
  const k = GRADE_KEY[(letter || "C")[0].toUpperCase()] || "c";
  return {
    tx: `var(--grade-${k}-tx)`,
    bg: `var(--grade-${k}-bg)`
  };
}
function Grade({
  value = "C",
  size = "md",
  style
}) {
  const {
    tx,
    bg
  } = gradeVars(value);
  const dims = size === "lg" ? {
    minWidth: 34,
    height: 30,
    fontSize: 18
  } : size === "sm" ? {
    minWidth: 22,
    height: 19,
    fontSize: 12
  } : {
    minWidth: 26,
    height: 22,
    fontSize: 14
  };
  return /*#__PURE__*/React.createElement("span", {
    style: {
      display: "inline-flex",
      alignItems: "center",
      justifyContent: "center",
      padding: "0 6px",
      fontFamily: "var(--font-mono)",
      fontWeight: 600,
      lineHeight: 1,
      border: "1px solid var(--hairline)",
      borderRadius: 2,
      color: tx,
      background: bg,
      ...dims,
      ...style
    }
  }, value);
}

// ---- Conviction meter — flat hairline scale ----
function Conviction({
  value = 0,
  of = 5,
  style
}) {
  return /*#__PURE__*/React.createElement("span", {
    style: {
      display: "inline-flex",
      gap: 3,
      alignItems: "center",
      ...style
    }
  }, Array.from({
    length: of
  }).map((_, i) => /*#__PURE__*/React.createElement("span", {
    key: i,
    style: {
      width: 14,
      height: 6,
      borderRadius: 1,
      display: "block",
      border: "1px solid " + (i < value ? "var(--ink-2)" : "var(--hairline)"),
      background: i < value ? "var(--ink-2)" : "transparent"
    }
  })));
}

// ---- Key-figure strip — hairline-delimited label-over-value ----
function KeyFigureStrip({
  items,
  style
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridAutoFlow: "column",
      gridAutoColumns: "1fr",
      border: "1px solid var(--hairline)",
      borderRadius: 2,
      background: "var(--paper)",
      ...style
    }
  }, items.map((it, i) => /*#__PURE__*/React.createElement("div", {
    key: i,
    style: {
      padding: "10px 14px",
      borderLeft: i === 0 ? "0" : "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)",
      marginBottom: 5,
      whiteSpace: "nowrap"
    }
  }, it.label), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: it.sans ? "var(--font-sans)" : "var(--font-mono)",
      fontVariantNumeric: "tabular-nums lining-nums",
      fontSize: it.sans ? 14 : 16,
      color: "var(--ink)",
      letterSpacing: "-0.01em"
    }
  }, it.value))));
}

// ---- Restrained sparkline — single ink weight, one accent series ----
function Sparkline({
  data = [],
  dir = "up",
  w = 120,
  h = 32,
  baseline = true
}) {
  if (!data.length) return null;
  const min = Math.min(...data),
    max = Math.max(...data);
  const span = max - min || 1;
  const P = 3;
  const x = i => P + i * ((w - 2 * P) / (data.length - 1));
  const y = v => P + (1 - (v - min) / span) * (h - 2 * P);
  const path = data.map((v, i) => (i ? "L" : "M") + x(i).toFixed(1) + " " + y(v).toFixed(1)).join(" ");
  const stroke = (DIR_META[dir] || DIR_META.up).color;
  return /*#__PURE__*/React.createElement("svg", {
    viewBox: `0 0 ${w} ${h}`,
    width: w,
    height: h,
    preserveAspectRatio: "none",
    style: {
      flexShrink: 0,
      display: "block"
    }
  }, baseline && /*#__PURE__*/React.createElement("line", {
    x1: "0",
    y1: h - P,
    x2: w,
    y2: h - P,
    stroke: "var(--hairline-soft)",
    strokeWidth: "0.5"
  }), /*#__PURE__*/React.createElement("path", {
    d: path,
    fill: "none",
    stroke: stroke,
    strokeWidth: "1.25"
  }));
}

// ---- Methodology affordance — restrained reveal ("how this was computed") ----
function Methodology({
  note,
  label = "how"
}) {
  const [open, setOpen] = React.useState(false);
  return /*#__PURE__*/React.createElement("span", {
    style: {
      position: "relative",
      display: "inline-block"
    }
  }, /*#__PURE__*/React.createElement("span", {
    onClick: () => setOpen(o => !o),
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      letterSpacing: "0.02em",
      color: "var(--ink-3)",
      borderBottom: "1px dotted var(--hairline)",
      cursor: "help",
      userSelect: "none"
    }
  }, label), open && /*#__PURE__*/React.createElement("span", {
    style: {
      position: "absolute",
      left: 0,
      top: "calc(100% + 6px)",
      zIndex: 10,
      width: 240,
      padding: "10px 12px",
      background: "var(--paper-edge)",
      border: "1px solid var(--hairline)",
      borderRadius: 2,
      fontFamily: "var(--font-serif)",
      fontSize: 12,
      lineHeight: 1.5,
      letterSpacing: "-0.006em",
      color: "var(--ink-2)"
    }
  }, note));
}

// ---- Card wrapper — flat hairline rectangle ----
function AnaCard({
  children,
  style
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      background: "var(--paper)",
      border: "1px solid var(--hairline)",
      borderRadius: 2,
      ...style
    }
  }, children);
}

// ---- Reveal disclosure — for falsifiers / triggers / lineage (density control) ----
function Reveal({
  label,
  children
}) {
  const [open, setOpen] = React.useState(false);
  return /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("button", {
    onClick: () => setOpen(o => !o),
    style: {
      display: "inline-flex",
      alignItems: "center",
      gap: 6,
      background: "transparent",
      border: 0,
      padding: "2px 0",
      cursor: "pointer",
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      fontWeight: 600,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 11
    }
  }, open ? "\u25BE" : "\u25B8"), label), open && /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 8
    }
  }, children));
}
Object.assign(window, {
  AnaHead,
  Dir,
  Grade,
  Conviction,
  KeyFigureStrip,
  Sparkline,
  Methodology,
  AnaCard,
  Reveal
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/Analytical.jsx", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/Icon.jsx
try { (() => {
// Icon.jsx — outlined, single-weight (1.25px at 20px), squared caps.
// One stroke, one color. No fills, no two-tone, no brand-color interiors.

function Icon({
  name,
  size = 16,
  color = "currentColor",
  style
}) {
  const sw = size <= 14 ? 1.1 : 1.25;
  const v = 20; // viewBox
  const common = {
    width: size,
    height: size,
    viewBox: `0 0 ${v} ${v}`,
    fill: "none",
    stroke: color,
    strokeWidth: sw,
    strokeLinecap: "square",
    strokeLinejoin: "miter",
    style: {
      display: "block",
      flexShrink: 0,
      ...style
    }
  };
  const paths = {
    report: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("rect", {
      x: "4",
      y: "3",
      width: "12",
      height: "14"
    }), /*#__PURE__*/React.createElement("line", {
      x1: "6.5",
      y1: "7",
      x2: "13.5",
      y2: "7"
    }), /*#__PURE__*/React.createElement("line", {
      x1: "6.5",
      y1: "10",
      x2: "13.5",
      y2: "10"
    }), /*#__PURE__*/React.createElement("line", {
      x1: "6.5",
      y1: "13",
      x2: "11.5",
      y2: "13"
    })),
    archive: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("rect", {
      x: "3",
      y: "3",
      width: "14",
      height: "3"
    }), /*#__PURE__*/React.createElement("rect", {
      x: "4",
      y: "6",
      width: "12",
      height: "11"
    }), /*#__PURE__*/React.createElement("line", {
      x1: "8",
      y1: "10",
      x2: "12",
      y2: "10"
    })),
    inbox: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("path", {
      d: "M3 11l3 -7h8l3 7v6H3z"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M3 11h4l1 2h4l1 -2h4"
    })),
    settings: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("circle", {
      cx: "10",
      cy: "10",
      r: "2.2"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M10 3v2 M10 15v2 M3 10h2 M15 10h2 M5 5l1.4 1.4 M13.6 13.6l1.4 1.4 M5 15l1.4 -1.4 M13.6 6.4l1.4 -1.4"
    })),
    warning: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("path", {
      d: "M10 4l7 12H3z"
    }), /*#__PURE__*/React.createElement("line", {
      x1: "10",
      y1: "9",
      x2: "10",
      y2: "13"
    }), /*#__PURE__*/React.createElement("line", {
      x1: "10",
      y1: "14.5",
      x2: "10",
      y2: "14.6"
    })),
    check: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("path", {
      d: "M4 11l4 4 8 -10"
    })),
    chevron_r: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("path", {
      d: "M8 5l5 5 -5 5"
    })),
    chevron_d: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("path", {
      d: "M5 8l5 5 5 -5"
    })),
    search: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("circle", {
      cx: "8.5",
      cy: "8.5",
      r: "4.5"
    }), /*#__PURE__*/React.createElement("line", {
      x1: "12",
      y1: "12",
      x2: "16",
      y2: "16"
    })),
    plus: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("line", {
      x1: "10",
      y1: "4",
      x2: "10",
      y2: "16"
    }), /*#__PURE__*/React.createElement("line", {
      x1: "4",
      y1: "10",
      x2: "16",
      y2: "10"
    })),
    export_: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("path", {
      d: "M10 13V3 M6 7l4 -4 4 4"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M3 13v4h14v-4"
    })),
    sidebar: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("rect", {
      x: "3",
      y: "4",
      width: "14",
      height: "12"
    }), /*#__PURE__*/React.createElement("line", {
      x1: "8",
      y1: "4",
      x2: "8",
      y2: "16"
    })),
    rule: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("line", {
      x1: "3",
      y1: "10",
      x2: "17",
      y2: "10"
    })),
    file: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("path", {
      d: "M5 3h7l3 3v11H5z"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M12 3v3h3"
    })),
    quote: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("path", {
      d: "M5 6h4v4H5z M5 10v3a2 2 0 002 2"
    }), /*#__PURE__*/React.createElement("path", {
      d: "M11 6h4v4h-4z M11 10v3a2 2 0 002 2"
    }))
  };
  return /*#__PURE__*/React.createElement("svg", common, paths[name] || null);
}
window.Icon = Icon;
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/Icon.jsx", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/LatestReport.jsx
try { (() => {
// LatestReport.jsx — The loosest, most generous surface in the system.
// A single readable column ~64ch, serif body, 8px baseline rhythm,
// hairlines between sections, watchlist + retrospective insets, and
// the rare display moments (title + dateline) at restrained sizes.

function ReportToolbar({
  title,
  dateline
}) {
  const [hover, setHover] = React.useState(null);
  const btnStyle = (key, primary) => ({
    display: "inline-flex",
    alignItems: "center",
    gap: 6,
    padding: "7px 12px",
    fontFamily: "var(--font-sans)",
    fontSize: 13,
    fontWeight: 500,
    whiteSpace: "nowrap",
    border: "1px solid " + (primary ? "var(--ink)" : "var(--hairline)"),
    background: primary ? hover === key ? "#2B241B" : "var(--ink)" : hover === key ? "var(--paper-soft)" : "transparent",
    color: primary ? "var(--paper)" : "var(--ink)",
    cursor: "pointer",
    borderRadius: 2,
    transition: "all 120ms cubic-bezier(0.4, 0.0, 0.2, 1)"
  });
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      padding: "10px 32px",
      borderBottom: "1px solid var(--hairline)",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)"
    }
  }, "Latest report"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 8
    }
  }, /*#__PURE__*/React.createElement("button", {
    onMouseEnter: () => setHover("pdf"),
    onMouseLeave: () => setHover(null),
    style: btnStyle("pdf", false)
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "export_",
    size: 13
  }), "Export PDF"), /*#__PURE__*/React.createElement("button", {
    onMouseEnter: () => setHover("share"),
    onMouseLeave: () => setHover(null),
    style: btnStyle("share", false)
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "file",
    size: 13
  }), "Share as Markdown")));
}
function SectionRule() {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      position: "relative",
      textAlign: "center",
      margin: "44px 0",
      color: "var(--ink-3)",
      fontFamily: "var(--font-serif)",
      fontSize: 14,
      lineHeight: 1
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      display: "inline-block",
      padding: "0 14px",
      background: "var(--paper)",
      position: "relative",
      zIndex: 1
    }
  }, "\u273B"), /*#__PURE__*/React.createElement("div", {
    style: {
      position: "absolute",
      left: 0,
      right: 0,
      top: "50%",
      borderTop: "1px solid var(--hairline)"
    }
  }));
}
function Figure({
  caption,
  source,
  height = 160,
  children
}) {
  return /*#__PURE__*/React.createElement("figure", {
    style: {
      margin: "32px 0",
      padding: 0,
      border: "1px solid var(--hairline)",
      borderRadius: 2,
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      height,
      padding: 16
    }
  }, children), /*#__PURE__*/React.createElement("figcaption", {
    style: {
      borderTop: "1px solid var(--hairline-soft)",
      padding: "8px 16px",
      display: "flex",
      justifyContent: "space-between",
      gap: 16
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-2)"
    }
  }, caption), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)"
    }
  }, "Source \xB7 ", source)));
}

// A restrained SVG figure — single ink color, hairline grid, one accent
// band when emphasis is needed. Not a dashboard widget.
function YieldChart() {
  // Two series, 26 points each
  const series1 = [3.62, 3.71, 3.68, 3.79, 3.88, 3.92, 4.01, 3.98, 4.05, 4.12, 4.21, 4.18, 4.24, 4.31, 4.28, 4.36, 4.41, 4.38, 4.45, 4.48, 4.42, 4.39, 4.34, 4.36, 4.29, 4.31];
  const series2 = [4.41, 4.48, 4.52, 4.57, 4.60, 4.62, 4.65, 4.66, 4.69, 4.71, 4.72, 4.71, 4.70, 4.69, 4.67, 4.66, 4.64, 4.62, 4.66, 4.69, 4.72, 4.74, 4.71, 4.73, 4.70, 4.69];
  const W = 720,
    H = 130,
    P = 8;
  const min = 3.5,
    max = 4.9;
  const x = i => P + i * ((W - 2 * P) / (series1.length - 1));
  const y = v => P + (1 - (v - min) / (max - min)) * (H - 2 * P);
  const path = s => s.map((v, i) => (i === 0 ? "M" : "L") + x(i) + " " + y(v)).join(" ");
  return /*#__PURE__*/React.createElement("svg", {
    viewBox: `0 0 ${W} ${H}`,
    width: "100%",
    height: "100%",
    preserveAspectRatio: "none"
  }, [0, 1, 2, 3].map(i => /*#__PURE__*/React.createElement("line", {
    key: i,
    x1: "0",
    x2: W,
    y1: P + i * (H - 2 * P) / 3,
    y2: P + i * (H - 2 * P) / 3,
    stroke: "#DCD4C0",
    strokeWidth: "0.5"
  })), /*#__PURE__*/React.createElement("path", {
    d: path(series2),
    stroke: "#1F1A14",
    strokeWidth: "1.25",
    fill: "none"
  }), /*#__PURE__*/React.createElement("path", {
    d: path(series1),
    stroke: "#6E2230",
    strokeWidth: "1.25",
    fill: "none"
  }), /*#__PURE__*/React.createElement("text", {
    x: W - 4,
    y: y(4.7) + 3,
    textAnchor: "end",
    fontFamily: "IBM Plex Mono",
    fontSize: "9",
    fill: "#7A6F5F"
  }, "4.70"), /*#__PURE__*/React.createElement("text", {
    x: W - 4,
    y: y(4.0) + 3,
    textAnchor: "end",
    fontFamily: "IBM Plex Mono",
    fontSize: "9",
    fill: "#7A6F5F"
  }, "4.00"), /*#__PURE__*/React.createElement("text", {
    x: W - 4,
    y: y(3.6) + 3,
    textAnchor: "end",
    fontFamily: "IBM Plex Mono",
    fontSize: "9",
    fill: "#7A6F5F"
  }, "3.60"), /*#__PURE__*/React.createElement("text", {
    x: x(25) - 4,
    y: y(series1[25]) - 5,
    textAnchor: "end",
    fontFamily: "Public Sans",
    fontSize: "10",
    fill: "#6E2230"
  }, "10Y \xB7 4.31%"), /*#__PURE__*/React.createElement("text", {
    x: x(25) - 4,
    y: y(series2[25]) + 12,
    textAnchor: "end",
    fontFamily: "Public Sans",
    fontSize: "10",
    fill: "#1F1A14"
  }, "2Y \xB7 4.69%"));
}
function Watchlist() {
  const {
    WATCHLIST
  } = window.MS_DATA;
  return /*#__PURE__*/React.createElement("div", {
    style: {
      margin: "32px 0",
      border: "1px solid var(--hairline)",
      borderRadius: 2,
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "space-between",
      padding: "10px 14px",
      borderBottom: "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-2)"
    }
  }, "Watchlist \xB7 this week"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)"
    }
  }, "Close \xB7 Fri Mar 29")), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "4px 14px 8px 14px"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1fr 100px 100px 100px",
      gap: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: tHeadCell()
  }, "Series"), /*#__PURE__*/React.createElement("div", {
    style: {
      ...tHeadCell(),
      textAlign: "right"
    }
  }, "Last"), /*#__PURE__*/React.createElement("div", {
    style: {
      ...tHeadCell(),
      textAlign: "right"
    }
  }, "\u0394 wk"), /*#__PURE__*/React.createElement("div", {
    style: {
      ...tHeadCell(),
      textAlign: "right"
    }
  }, "\u0394 ytd"), WATCHLIST.map((r, i) => {
    const up = r.wk.startsWith("+");
    const flat = r.wk === "0.00%" || r.wk === "+0.00";
    const ch = flat ? "·" : up ? "▴" : "▾";
    return /*#__PURE__*/React.createElement(React.Fragment, {
      key: r.name
    }, /*#__PURE__*/React.createElement("div", {
      style: tCell(i === WATCHLIST.length - 1)
    }, r.name), /*#__PURE__*/React.createElement("div", {
      style: {
        ...tCell(i === WATCHLIST.length - 1, true),
        textAlign: "right"
      }
    }, r.last), /*#__PURE__*/React.createElement("div", {
      style: {
        ...tCell(i === WATCHLIST.length - 1, true),
        textAlign: "right",
        display: "flex",
        justifyContent: "flex-end",
        gap: 5
      }
    }, /*#__PURE__*/React.createElement("span", {
      style: {
        color: "var(--ink-2)"
      }
    }, ch), /*#__PURE__*/React.createElement("span", null, r.wk.replace("−", "").replace("+", ""))), /*#__PURE__*/React.createElement("div", {
      style: {
        ...tCell(i === WATCHLIST.length - 1, true),
        textAlign: "right",
        color: "var(--ink-2)"
      }
    }, r.ytd));
  }))));
}
function tHeadCell() {
  return {
    fontFamily: "var(--font-sans)",
    fontSize: 10,
    letterSpacing: "0.08em",
    textTransform: "uppercase",
    color: "var(--ink-3)",
    padding: "8px 6px",
    borderBottom: "1px solid var(--hairline)"
  };
}
function tCell(last, mono) {
  return {
    fontFamily: mono ? "var(--font-mono)" : "var(--font-sans)",
    fontSize: 13,
    fontVariantNumeric: "tabular-nums lining-nums",
    color: "var(--ink)",
    padding: "8px 6px",
    borderBottom: last ? "none" : "1px solid var(--hairline-soft)"
  };
}
function Retrospective() {
  return /*#__PURE__*/React.createElement("aside", {
    style: {
      margin: "32px 0",
      borderTop: "1px solid var(--ink)",
      borderBottom: "1px solid var(--hairline)",
      paddingTop: 12,
      paddingBottom: 16
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.08em",
      textTransform: "uppercase",
      color: "var(--ink-3)",
      marginBottom: 8
    }
  }, "Retrospective \xB7 graded from issue 140"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 17,
      lineHeight: 1.55,
      letterSpacing: "-0.006em",
      color: "var(--ink)"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontStyle: "italic",
      color: "var(--ink-2)"
    }
  }, "\"Energy is being structurally re-rated, and the move has further to run.\""), /*#__PURE__*/React.createElement("span", null, " \u2014 that call looks early. The underlying logic still holds, but the timing was wrong. WTI is flat on the four weeks since the issue. We continue to hold the thesis; we are no longer holding the timing.")));
}
function AnalystVoices() {
  const voices = [{
    name: "Bull",
    stance: "long energy here",
    text: "Capex discipline is binding; the marginal barrel is no longer being underwritten. The re-rating completes inside twelve months."
  }, {
    name: "Bear",
    stance: "premature",
    text: "Demand destruction is doing more work than the consensus expects. The 2014 analogue is more relevant than the 2007 one."
  }, {
    name: "Balanced",
    stance: "right thesis, wrong window",
    text: "Hold the thesis at the issue level; do not hold the timing. Conditions for revision are below."
  }];
  return /*#__PURE__*/React.createElement("section", {
    style: {
      margin: "32px 0",
      border: "1px solid var(--hairline)",
      borderRadius: 2
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "10px 14px",
      borderBottom: "1px solid var(--hairline-soft)",
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.08em",
      textTransform: "uppercase",
      color: "var(--ink-3)"
    }
  }, "Internal stress-test \xB7 three voices"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1fr 1fr 1fr"
    }
  }, voices.map((v, i) => /*#__PURE__*/React.createElement("div", {
    key: v.name,
    style: {
      padding: "14px 16px",
      borderRight: i < 2 ? "1px solid var(--hairline-soft)" : "none"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      fontWeight: 600,
      color: "var(--ink)",
      letterSpacing: 0
    }
  }, v.name), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)",
      marginTop: 2,
      marginBottom: 8
    }
  }, v.stance), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 14,
      lineHeight: 1.5,
      letterSpacing: "-0.006em",
      color: "var(--ink-2)"
    }
  }, v.text)))));
}
function LatestReport({
  report
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflowY: "auto",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement(ReportToolbar, null), /*#__PURE__*/React.createElement("article", {
    style: {
      maxWidth: 720,
      margin: "0 auto",
      padding: "56px 32px 96px 32px"
    }
  }, /*#__PURE__*/React.createElement("header", {
    style: {
      marginBottom: 32
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.08em",
      textTransform: "uppercase",
      color: "var(--ink-3)",
      marginBottom: 14
    }
  }, "Issue ", report.id, " \xB7 weekly"), /*#__PURE__*/React.createElement("h1", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 32,
      lineHeight: 1.18,
      fontWeight: 600,
      letterSpacing: 0,
      color: "var(--ink)",
      margin: "0 0 6px 0"
    }
  }, report.title), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-serif)",
      fontStyle: "italic",
      fontSize: 15,
      color: "var(--ink-3)"
    }
  }, report.date, " \xB7 9 minutes")), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 17,
      lineHeight: 1.55,
      letterSpacing: "-0.006em",
      color: "var(--ink)"
    }
  }, /*#__PURE__*/React.createElement("p", {
    style: {
      margin: "0 0 16px 0"
    }
  }, "The thesis is unchanged this week. We are not raising the energy call, we are not lowering it, and we are not adding a new one. Equity markets drifted; rates drifted; the dollar drifted. There is no story to write that did not already exist seven days ago."), /*#__PURE__*/React.createElement("p", {
    style: {
      margin: "0 0 16px 0"
    }
  }, "That is, by itself, worth noting. The first quarter delivered three regime-adjacent moves \u2014 the late-January reversal in rate-cut expectations, the early-March repricing of energy, and the quiet revision of the soft-landing consensus. None of them extended this week. We are watching for the conditions under which they would extend; below."), /*#__PURE__*/React.createElement(SectionRule, null), /*#__PURE__*/React.createElement("h2", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 22,
      lineHeight: 1.3,
      fontWeight: 600,
      color: "var(--ink)",
      margin: "0 0 12px 0"
    }
  }, "\xA71 \u2014 State of play"), /*#__PURE__*/React.createElement("p", {
    style: {
      margin: "0 0 16px 0"
    }
  }, "Front-end yields are anchored around 4.69%; the back end, around 4.31%. The curve is flatter than at any point since November, and the flattening has come almost entirely from the long end. We continue to read this as the bond market grading the Committee's projected path against the prints, rather than as a fundamental shift in real-rate expectations."), /*#__PURE__*/React.createElement(Figure, {
    caption: "Figure 1 \xB7 2-yr and 10-yr Treasury yields",
    source: "FRED"
  }, /*#__PURE__*/React.createElement(YieldChart, null)), /*#__PURE__*/React.createElement("p", {
    style: {
      margin: "0 0 16px 0"
    }
  }, "We pay attention to the 10-year (in oxblood, above) because that is where the re-rating, if it happens, will show up first."), /*#__PURE__*/React.createElement(Watchlist, null), /*#__PURE__*/React.createElement(SectionRule, null), /*#__PURE__*/React.createElement("h2", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 22,
      lineHeight: 1.3,
      fontWeight: 600,
      color: "var(--ink)",
      margin: "0 0 12px 0"
    }
  }, "\xA72 \u2014 Last month's energy call, graded"), /*#__PURE__*/React.createElement(Retrospective, null), /*#__PURE__*/React.createElement("p", {
    style: {
      margin: "0 0 16px 0"
    }
  }, "We pulled the trigger early. The structural argument \u2014 capex discipline, the unwillingness of the marginal producer to underwrite the marginal barrel \u2014 is still the right argument. The four-week tape is not validating it. We are not changing the issue-level thesis; we are flagging that issue 140 should not be read as a tactical recommendation."), /*#__PURE__*/React.createElement(SectionRule, null), /*#__PURE__*/React.createElement("h2", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 22,
      lineHeight: 1.3,
      fontWeight: 600,
      color: "var(--ink)",
      margin: "0 0 12px 0"
    }
  }, "\xA73 \u2014 Stress-test of the open thesis"), /*#__PURE__*/React.createElement(AnalystVoices, null), /*#__PURE__*/React.createElement("p", {
    style: {
      margin: "0 0 16px 0"
    }
  }, "The Balanced read is the one we will continue to publish under our own byline. The Bull case is interesting and is on the record above; the Bear case is the one we cannot fully refute, and so it sits next to the thesis as a permanent caveat."), /*#__PURE__*/React.createElement(SectionRule, null), /*#__PURE__*/React.createElement("h2", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 22,
      lineHeight: 1.3,
      fontWeight: 600,
      color: "var(--ink)",
      margin: "0 0 12px 0"
    }
  }, "\xA74 \u2014 What would force a revision"), /*#__PURE__*/React.createElement("p", {
    style: {
      margin: "0 0 16px 0"
    }
  }, "Two things, named in advance so they are not retrofitted:"), /*#__PURE__*/React.createElement("ul", {
    style: {
      margin: "0 0 16px 0",
      paddingLeft: 20,
      listStyle: "square"
    }
  }, /*#__PURE__*/React.createElement("li", {
    style: {
      marginBottom: 8
    }
  }, "A sustained breach of ", /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontVariantNumeric: "tabular-nums"
    }
  }, "$74"), " on the front-month crude contract, held for two consecutive weekly closes."), /*#__PURE__*/React.createElement("li", null, "A clear inflection in core services inflation in either direction \u2014 specifically, a three-month annualized print outside the 3.4\u20134.2% corridor.")), /*#__PURE__*/React.createElement("p", {
    style: {
      margin: "0 0 16px 0"
    }
  }, "Neither has happened. The thesis is unchanged."), /*#__PURE__*/React.createElement(SectionRule, null), /*#__PURE__*/React.createElement("p", {
    style: {
      margin: "32px 0 0 0",
      textAlign: "left",
      fontFamily: "var(--font-serif)",
      fontStyle: "italic",
      color: "var(--ink-3)",
      fontSize: 14
    }
  }, "\u2014 Market Signal \xB7 Sunday, March 31"))));
}
Object.assign(window, {
  LatestReport,
  SectionRule,
  Figure,
  YieldChart,
  Watchlist,
  Retrospective,
  AnalystVoices,
  ReportToolbar
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/LatestReport.jsx", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/Portfolio.jsx
try { (() => {
// Portfolio.jsx — Portfolio Analysis surface (analytical register).
// Two-step trigger (Pull holdings -> Run analysis), holdings as cards
// (full / reduced / not-rated / insufficient), and a whole-book roll-up.
// Layout/IA per the brief; restyle, not re-architect.

// ---- Standing-thesis anchor — handles a long thesis with graceful overflow ----
function ThesisAnchor({
  text,
  lead = true
}) {
  const [open, setOpen] = React.useState(false);
  const [overflows, setOverflows] = React.useState(false);
  const ref = React.useRef(null);
  React.useLayoutEffect(() => {
    const el = ref.current;
    if (el) setOverflows(el.scrollHeight - el.clientHeight > 2);
  }, [text]);
  return /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      marginBottom: 6
    }
  }, "Standing thesis"), /*#__PURE__*/React.createElement("p", {
    ref: ref,
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: lead ? 15 : 14,
      lineHeight: 1.5,
      letterSpacing: "-0.006em",
      color: "var(--ink)",
      margin: 0,
      ...(!open ? {
        display: "-webkit-box",
        WebkitLineClamp: 3,
        WebkitBoxOrient: "vertical",
        overflow: "hidden"
      } : {})
    }
  }, text), (overflows || open) && /*#__PURE__*/React.createElement("button", {
    onClick: () => setOpen(o => !o),
    style: {
      marginTop: 6,
      background: "transparent",
      border: 0,
      padding: 0,
      cursor: "pointer",
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      fontWeight: 600,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--accent)"
    }
  }, open ? "Show less" : "Read full thesis"));
}
function SubScores({
  sub
}) {
  const entries = Object.entries(sub);
  const labelMap = {
    quality: "Qual",
    valuation: "Val",
    momentum: "Mom",
    risk: "Risk",
    exposure: "Expo",
    houseView: "House"
  };
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: `repeat(${entries.length}, 1fr)`,
      gap: "0 4px"
    }
  }, entries.map(([k, v]) => /*#__PURE__*/React.createElement("div", {
    key: k
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 9,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)",
      marginBottom: 4
    }
  }, labelMap[k] || k), /*#__PURE__*/React.createElement(Grade, {
    value: v,
    size: "sm"
  }))));
}
function KV({
  rows
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "max-content 1fr",
      columnGap: 14,
      rowGap: 5,
      fontFamily: "var(--font-sans)",
      fontSize: 12
    }
  }, rows.map((r, i) => /*#__PURE__*/React.createElement(React.Fragment, {
    key: i
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      color: "var(--ink-3)",
      whiteSpace: "nowrap"
    }
  }, r.k), /*#__PURE__*/React.createElement("div", {
    style: {
      color: "var(--ink)"
    }
  }, r.v))));
}
function Scenarios({
  rows
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "repeat(3, 1fr)",
      borderTop: "1px solid var(--hairline-soft)"
    }
  }, rows.map((s, i) => /*#__PURE__*/React.createElement("div", {
    key: s.k,
    style: {
      padding: "10px 14px",
      borderRight: i < 2 ? "1px solid var(--hairline-soft)" : "none"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "space-between",
      alignItems: "baseline",
      marginBottom: 4
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      color: "var(--ink-2)"
    }
  }, s.k), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      color: "var(--ink-3)"
    }
  }, s.p)), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 14,
      color: "var(--ink)",
      fontVariantNumeric: "tabular-nums"
    }
  }, s.t), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 12,
      lineHeight: 1.4,
      color: "var(--ink-3)",
      marginTop: 3
    }
  }, s.note))));
}
function ClassTag({
  klass,
  state
}) {
  const map = {
    "rated": "Stock · full verdict",
    "rated-reduced": "ETF · reduced verdict",
    "not-rated": "Not rated",
    "insufficient": "Insufficient evidence"
  };
  const klassMap = {
    stock: "Stock",
    etf: "ETF / fund",
    option: "Options",
    cash: "Cash",
    unsupported: "Unsupported"
  };
  return /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      color: "var(--ink-3)",
      whiteSpace: "nowrap"
    }
  }, state === "rated" ? map.rated : state === "rated-reduced" ? map["rated-reduced"] : (klassMap[klass] || klass) + " · " + map[state]);
}
function HoldingCard({
  h
}) {
  // Not-rated / insufficient — short reason, no grade, reads as legitimately reduced.
  if (h.state === "not-rated" || h.state === "insufficient") {
    const abst = h.state === "insufficient";
    return /*#__PURE__*/React.createElement(AnaCard, {
      style: {
        borderColor: "var(--hairline)",
        background: abst ? "var(--paper)" : "var(--paper)"
      }
    }, /*#__PURE__*/React.createElement("div", {
      style: {
        display: "flex",
        justifyContent: "space-between",
        alignItems: "flex-start",
        gap: 16,
        padding: "14px 18px"
      }
    }, /*#__PURE__*/React.createElement("div", {
      style: {
        minWidth: 0
      }
    }, /*#__PURE__*/React.createElement("div", {
      style: {
        display: "flex",
        alignItems: "baseline",
        gap: 10,
        marginBottom: 6
      }
    }, /*#__PURE__*/React.createElement("span", {
      style: {
        fontFamily: "var(--font-mono)",
        fontWeight: 500,
        fontSize: 15,
        letterSpacing: "0.02em",
        color: "var(--ink)"
      }
    }, h.ticker), /*#__PURE__*/React.createElement(ClassTag, {
      klass: h.klass,
      state: h.state
    })), /*#__PURE__*/React.createElement("p", {
      style: {
        fontFamily: "var(--font-serif)",
        fontSize: 13,
        lineHeight: 1.45,
        color: "var(--ink-2)",
        margin: 0,
        maxWidth: "70ch"
      }
    }, h.reason)), /*#__PURE__*/React.createElement("div", {
      style: {
        textAlign: "right",
        flexShrink: 0
      }
    }, /*#__PURE__*/React.createElement(AnaHead, {
      style: {
        color: "var(--ink-3)",
        marginBottom: 3
      }
    }, "Weight"), /*#__PURE__*/React.createElement("span", {
      style: {
        fontFamily: "var(--font-mono)",
        fontSize: 14,
        color: "var(--ink-2)",
        fontVariantNumeric: "tabular-nums"
      }
    }, h.weight))));
  }
  const reduced = h.state === "rated-reduced";
  return /*#__PURE__*/React.createElement(AnaCard, null, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "flex-start",
      justifyContent: "space-between",
      gap: 16,
      padding: "16px 18px 14px",
      borderBottom: "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 12,
      minWidth: 0
    }
  }, /*#__PURE__*/React.createElement(Grade, {
    value: h.grade,
    size: "lg"
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      minWidth: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "baseline",
      gap: 8
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontWeight: 500,
      fontSize: 15,
      letterSpacing: "0.02em",
      color: "var(--ink)"
    }
  }, h.ticker), /*#__PURE__*/React.createElement(ClassTag, {
    klass: h.klass,
    state: h.state
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 12,
      color: "var(--ink-3)",
      marginTop: 1
    }
  }, h.name, " \xB7 ", h.sector))), /*#__PURE__*/React.createElement("div", {
    style: {
      textAlign: "right",
      flexShrink: 0
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      color: "var(--ink-3)",
      marginBottom: 3
    }
  }, "Unrealized"), /*#__PURE__*/React.createElement(Dir, {
    dir: h.unrealized.dir,
    size: 16
  }, h.unrealized.val))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "14px 18px",
      borderBottom: "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement(ThesisAnchor, {
    text: h.thesis
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1fr 1fr"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "14px 18px",
      borderRight: "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      marginBottom: 10
    }
  }, reduced ? "Intrinsic verdict · reduced" : "Intrinsic verdict"), /*#__PURE__*/React.createElement("div", {
    style: {
      marginBottom: 12
    }
  }, /*#__PURE__*/React.createElement(SubScores, {
    sub: h.sub
  })), /*#__PURE__*/React.createElement(KV, {
    rows: [{
      k: "Conviction",
      v: /*#__PURE__*/React.createElement(Conviction, {
        value: h.conviction
      })
    }, ...(reduced ? [] : [{
      k: "EOM target",
      v: /*#__PURE__*/React.createElement("span", {
        style: {
          fontFamily: "var(--font-mono)",
          fontVariantNumeric: "tabular-nums"
        }
      }, h.eom, " ", /*#__PURE__*/React.createElement(Methodology, {
        note: "End-of-month target: DCF fair value bridged to a 1-month multiple path. Deterministic; same inputs, same number."
      }))
    }, {
      k: "EOY target",
      v: /*#__PURE__*/React.createElement("span", {
        style: {
          fontFamily: "var(--font-mono)",
          fontVariantNumeric: "tabular-nums"
        }
      }, h.eoy)
    }]), {
      k: "Standalone",
      v: h.standalone
    }]
  }), h.health && /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 12,
      lineHeight: 1.45,
      color: "var(--ink-3)",
      marginTop: 10
    }
  }, h.health)), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "14px 18px"
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      marginBottom: 10
    }
  }, "Portfolio action"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "baseline",
      gap: 8,
      marginBottom: 10
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 15,
      fontWeight: 600,
      color: "var(--ink)"
    }
  }, h.action), /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      color: "var(--ink-3)",
      whiteSpace: "nowrap"
    }
  }, "to ", h.targetWeight)), /*#__PURE__*/React.createElement("div", {
    style: {
      marginBottom: 10
    }
  }, /*#__PURE__*/React.createElement(KV, {
    rows: [{
      k: "Weight",
      v: /*#__PURE__*/React.createElement("span", {
        style: {
          fontFamily: "var(--font-mono)",
          fontVariantNumeric: "tabular-nums"
        }
      }, h.weight)
    }, {
      k: "Est. adj.",
      v: /*#__PURE__*/React.createElement("span", {
        style: {
          fontFamily: "var(--font-mono)",
          fontVariantNumeric: "tabular-nums"
        }
      }, h.adj)
    }]
  })), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 13,
      lineHeight: 1.45,
      letterSpacing: "-0.006em",
      color: "var(--ink-2)",
      margin: 0
    }
  }, h.rationale))), h.scenarios && /*#__PURE__*/React.createElement(Scenarios, {
    rows: h.scenarios
  }), (h.triggers || h.deadMoney) && /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "12px 18px",
      borderTop: "1px solid var(--hairline-soft)"
    }
  }, h.deadMoney && /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 8,
      marginBottom: h.triggers ? 10 : 0
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      color: "var(--ana-down)",
      whiteSpace: "nowrap"
    }
  }, "Capital-efficiency"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 12,
      color: "var(--ink-2)"
    }
  }, h.deadMoney)), h.triggers && /*#__PURE__*/React.createElement(Reveal, {
    label: "Triggers & falsifiers"
  }, /*#__PURE__*/React.createElement(KV, {
    rows: [{
      k: "Add",
      v: h.triggers.add
    }, {
      k: "Trim",
      v: h.triggers.trim
    }, {
      k: "Sell",
      v: h.triggers.sell
    }]
  }))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      gap: 16,
      padding: "12px 18px",
      borderTop: "1px solid var(--hairline-soft)",
      background: "var(--paper-edge)"
    }
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      color: "var(--ink-3)",
      marginBottom: 3
    }
  }, "What changed \xB7 since last run"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 12,
      color: "var(--ink-2)"
    }
  }, "Intrinsic ", /*#__PURE__*/React.createElement("span", {
    style: {
      color: "var(--ink)"
    }
  }, h.changed.intrinsic), " \xB7 action ", /*#__PURE__*/React.createElement("span", {
    style: {
      color: "var(--ink)"
    }
  }, h.changed.action), " \xB7 position ", /*#__PURE__*/React.createElement("span", {
    style: {
      color: "var(--ink)"
    }
  }, h.changed.position))), h.curve && /*#__PURE__*/React.createElement(Sparkline, {
    data: h.curve,
    dir: h.unrealized.dir
  })));
}

// ---- Whole-book roll-up & construction panel ----
function ConstructionPanel({
  book
}) {
  return /*#__PURE__*/React.createElement(AnaCard, {
    style: {
      marginTop: 24
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "14px 18px",
      borderBottom: "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      marginBottom: 8
    }
  }, "Roll-up & construction \xB7 whole book"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 8,
      alignItems: "baseline"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 15,
      fontWeight: 600,
      color: "var(--ink)"
    }
  }, "Risk posture: ", book.posture)), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 13,
      lineHeight: 1.5,
      color: "var(--ink-2)",
      margin: "8px 0 0",
      maxWidth: "78ch"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontWeight: 600,
      color: "var(--ink)"
    }
  }, "Cash & deployment."), " ", book.cash)), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "12px 18px",
      borderBottom: "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      marginBottom: 8
    }
  }, "Concentration & exposure"), /*#__PURE__*/React.createElement("table", {
    style: {
      width: "100%",
      borderCollapse: "collapse"
    }
  }, /*#__PURE__*/React.createElement("thead", null, /*#__PURE__*/React.createElement("tr", null, ["Cluster", "Weight", "Names", "β-contrib", "Δ run"].map((c, i) => /*#__PURE__*/React.createElement("th", {
    key: c,
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      fontWeight: 600,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)",
      textAlign: i === 0 ? "left" : "right",
      padding: "6px 8px",
      borderBottom: "1px solid var(--hairline)",
      whiteSpace: "nowrap"
    }
  }, c)))), /*#__PURE__*/React.createElement("tbody", null, book.concentration.map((r, i) => /*#__PURE__*/React.createElement("tr", {
    key: r.cluster
  }, /*#__PURE__*/React.createElement("td", {
    style: {
      fontSize: 12,
      color: "var(--ink)",
      padding: "6px 8px",
      borderBottom: i < book.concentration.length - 1 ? "1px solid var(--hairline-soft)" : "none"
    }
  }, r.cluster), /*#__PURE__*/React.createElement("td", {
    style: tdNum(i, book.concentration.length)
  }, r.weight), /*#__PURE__*/React.createElement("td", {
    style: tdNum(i, book.concentration.length)
  }, r.names), /*#__PURE__*/React.createElement("td", {
    style: tdNum(i, book.concentration.length)
  }, r.beta), /*#__PURE__*/React.createElement("td", {
    style: {
      ...tdNum(i, book.concentration.length),
      textAlign: "right"
    }
  }, /*#__PURE__*/React.createElement(Dir, {
    dir: r.delta.dir
  }, r.delta.val))))))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "12px 18px",
      borderBottom: "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      marginBottom: 8
    }
  }, "Overlap clusters"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 10
    }
  }, book.overlap.map(o => /*#__PURE__*/React.createElement("div", {
    key: o.name
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 10,
      alignItems: "baseline",
      flexWrap: "wrap"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      fontWeight: 600,
      color: "var(--ink)"
    }
  }, o.name), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      color: "var(--ink-3)"
    }
  }, o.holdings)), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 12,
      lineHeight: 1.45,
      color: "var(--ink-2)",
      marginTop: 2
    }
  }, o.note))))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1fr 1fr"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "12px 18px",
      borderRight: "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      marginBottom: 8
    }
  }, "Positions closed since last run"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 8
    }
  }, book.closed.map(c => /*#__PURE__*/React.createElement("div", {
    key: c.ticker,
    style: {
      display: "flex",
      gap: 10
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 12,
      color: "var(--ink)",
      minWidth: 36
    }
  }, c.ticker), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 12,
      lineHeight: 1.45,
      color: "var(--ink-2)"
    }
  }, c.note))))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "12px 18px"
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      marginBottom: 8
    }
  }, "Not-rated risk contribution"), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 12,
      lineHeight: 1.45,
      color: "var(--ink-2)",
      margin: 0
    }
  }, book.notRatedRisk))));
}
function tdNum(i, len) {
  return {
    fontFamily: "var(--font-mono)",
    fontSize: 12,
    fontVariantNumeric: "tabular-nums",
    color: "var(--ink)",
    textAlign: "right",
    padding: "6px 8px",
    borderBottom: i < len - 1 ? "1px solid var(--hairline-soft)" : "none"
  };
}

// ---- Trigger controls (two-step) ----
function PortfolioToolbar({
  phase,
  onPull,
  onRun
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      padding: "10px 32px",
      borderBottom: "1px solid var(--hairline)",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement(AnaHead, null, "Portfolio analysis"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 8,
      alignItems: "center"
    }
  }, /*#__PURE__*/React.createElement("button", {
    onClick: onPull,
    style: btnStyle(phase !== "empty")
  }, "1 \xB7 Pull holdings"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 12,
      color: "var(--ink-3)"
    }
  }, "\u2192"), /*#__PURE__*/React.createElement("button", {
    onClick: onRun,
    disabled: phase === "empty",
    style: btnStyle(phase === "pulled", phase === "empty")
  }, "2 \xB7 Run analysis")));
}
function btnStyle(primary, disabled) {
  return {
    display: "inline-flex",
    alignItems: "center",
    gap: 6,
    padding: "7px 12px",
    fontFamily: "var(--font-sans)",
    fontSize: 13,
    fontWeight: 500,
    whiteSpace: "nowrap",
    border: "1px solid " + (disabled ? "var(--hairline)" : "var(--ink)"),
    background: primary && !disabled ? "var(--ink)" : "transparent",
    color: disabled ? "var(--ink-3)" : primary ? "var(--paper)" : "var(--ink)",
    cursor: disabled ? "default" : "pointer",
    borderRadius: 2,
    transition: "all 120ms cubic-bezier(0.4, 0.0, 0.2, 1)"
  };
}
function EmptyPortfolio({
  phase,
  onPull,
  onRun
}) {
  const pulled = phase === "pulled";
  return /*#__PURE__*/React.createElement("div", {
    style: {
      maxWidth: 720,
      margin: "0 auto",
      padding: "72px 32px"
    }
  }, /*#__PURE__*/React.createElement("h2", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 22,
      fontWeight: 600,
      color: "var(--ink)",
      margin: "0 0 8px"
    }
  }, pulled ? "23 holdings pulled. Not yet analyzed." : "No holdings pulled yet."), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 15,
      lineHeight: 1.55,
      color: "var(--ink-2)",
      margin: "0 0 24px",
      maxWidth: "60ch"
    }
  }, pulled ? "Holdings were fetched from your connected Schwab account. Run the analysis to grade them; nothing is graded until you ask." : "Holdings are fetched only on explicit action — never auto-synced. Pull from your connected Schwab account, then run the analysis."), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 10,
      marginBottom: 32
    }
  }, /*#__PURE__*/React.createElement("button", {
    onClick: onPull,
    style: btnStyle(!pulled)
  }, "Pull holdings"), /*#__PURE__*/React.createElement("button", {
    onClick: onRun,
    disabled: !pulled,
    style: btnStyle(pulled, !pulled)
  }, "Run analysis")), /*#__PURE__*/React.createElement("div", {
    style: {
      borderTop: "1px solid var(--hairline)",
      paddingTop: 16,
      maxWidth: "60ch"
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      marginBottom: 6
    }
  }, "Supplement \xB7 manual import"), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 13,
      lineHeight: 1.5,
      color: "var(--ink-3)",
      margin: 0
    }
  }, "Paste symbols, quantities, and cost bases \u2014 or drop a CSV \u2014 to add positions Schwab does not report. This supplements the pull; it does not replace the Schwab connection, which gates the job regardless.")));
}
function Portfolio() {
  const {
    BOOK,
    HOLDINGS
  } = window.MS_DATA;
  const [phase, setPhase] = React.useState("ran"); // empty | pulled | ran

  if (phase !== "ran") {
    return /*#__PURE__*/React.createElement("div", {
      style: {
        flex: 1,
        overflowY: "auto",
        background: "var(--paper)"
      }
    }, /*#__PURE__*/React.createElement(PortfolioToolbar, {
      phase: phase,
      onPull: () => setPhase("pulled"),
      onRun: () => setPhase("ran")
    }), /*#__PURE__*/React.createElement(EmptyPortfolio, {
      phase: phase,
      onPull: () => setPhase("pulled"),
      onRun: () => setPhase("ran")
    }));
  }
  return /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflowY: "auto",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement(PortfolioToolbar, {
    phase: phase,
    onPull: () => setPhase("pulled"),
    onRun: () => setPhase("ran")
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      maxWidth: 980,
      margin: "0 auto",
      padding: "24px 32px 96px"
    }
  }, /*#__PURE__*/React.createElement(KeyFigureStrip, {
    items: [{
      label: "Book value",
      value: BOOK.value
    }, {
      label: "Holdings",
      value: BOOK.holdings
    }, {
      label: "Rated",
      value: BOOK.rated
    }, {
      label: "Not rated",
      value: BOOK.notRated
    }, {
      label: "Since last run",
      value: /*#__PURE__*/React.createElement(Dir, {
        dir: BOOK.sinceRun.dir,
        size: 16
      }, BOOK.sinceRun.val)
    }, {
      label: "Posture",
      value: BOOK.posture,
      sans: true
    }],
    style: {
      marginBottom: 24
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 16
    }
  }, HOLDINGS.map(h => /*#__PURE__*/React.createElement(HoldingCard, {
    key: h.ticker,
    h: h
  }))), /*#__PURE__*/React.createElement(ConstructionPanel, {
    book: BOOK
  })));
}
Object.assign(window, {
  Portfolio,
  HoldingCard,
  ConstructionPanel,
  ThesisAnchor
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/Portfolio.jsx", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/ResearchInbox.jsx
try { (() => {
// ResearchInbox.jsx — user-supplied PDFs and notes, organized for later
// citation. Dense, single-column list. No bulk action chrome.

function InboxToolbar() {
  const [hover, setHover] = React.useState(null);
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      padding: "10px 32px",
      borderBottom: "1px solid var(--hairline)",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)"
    }
  }, "Research inbox"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 8
    }
  }, /*#__PURE__*/React.createElement("button", {
    onMouseEnter: () => setHover("add"),
    onMouseLeave: () => setHover(null),
    style: {
      display: "inline-flex",
      alignItems: "center",
      gap: 6,
      padding: "7px 12px",
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      fontWeight: 500,
      whiteSpace: "nowrap",
      border: "1px solid var(--ink)",
      background: hover === "add" ? "#2B241B" : "var(--ink)",
      color: "var(--paper)",
      cursor: "pointer",
      borderRadius: 2,
      transition: "all 120ms cubic-bezier(0.4, 0.0, 0.2, 1)"
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "plus",
    size: 13
  }), "Add file or note")));
}
function InboxRow({
  item
}) {
  const [hover, setHover] = React.useState(false);
  return /*#__PURE__*/React.createElement("div", {
    onMouseEnter: () => setHover(true),
    onMouseLeave: () => setHover(false),
    style: {
      display: "grid",
      gridTemplateColumns: "20px 1fr max-content max-content",
      gap: 14,
      alignItems: "baseline",
      padding: "12px 32px",
      borderBottom: "1px solid var(--hairline-soft)",
      background: hover ? "var(--paper-soft)" : "transparent",
      cursor: "pointer",
      transition: "background-color 120ms cubic-bezier(0.4, 0.0, 0.2, 1)"
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "file",
    size: 14,
    color: "var(--ink-2)"
  }), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 14,
      color: "var(--ink)",
      fontWeight: 500
    }
  }, item.title), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)",
      marginTop: 2
    }
  }, item.source, " \xB7 ", item.tag)), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontVariantNumeric: "tabular-nums",
      fontSize: 12,
      color: "var(--ink-3)"
    }
  }, "added ", item.added), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontVariantNumeric: "tabular-nums",
      fontSize: 12,
      color: "var(--ink-3)"
    }
  }, "#", item.id));
}
function ResearchInbox() {
  const {
    INBOX_ITEMS
  } = window.MS_DATA;
  return /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflowY: "auto",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement(InboxToolbar, null), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "28px 32px 16px 32px",
      maxWidth: 920
    }
  }, /*#__PURE__*/React.createElement("h2", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 22,
      lineHeight: 1.3,
      fontWeight: 600,
      color: "var(--ink)",
      margin: "0 0 6px 0"
    }
  }, "Filed research"), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 15,
      lineHeight: 1.5,
      letterSpacing: "-0.006em",
      color: "var(--ink-2)",
      margin: "0 0 20px 0",
      maxWidth: "62ch"
    }
  }, "Drop PDFs, transcripts, or text notes into this folder and the analyst pipeline will consider them when writing next week's issue. Nothing is sent to a third party until you generate.")), /*#__PURE__*/React.createElement("div", {
    style: {
      borderTop: "1px solid var(--hairline)",
      borderBottom: "1px solid var(--hairline)"
    }
  }, INBOX_ITEMS.map(item => /*#__PURE__*/React.createElement(InboxRow, {
    key: item.id,
    item: item
  }))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "16px 32px",
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)"
    }
  }, INBOX_ITEMS.length, " items \xB7 all local \xB7 last sync \u2014"));
}
Object.assign(window, {
  ResearchInbox,
  InboxRow
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/ResearchInbox.jsx", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/RunTracker.jsx
try { (() => {
// RunTracker.jsx — the one run tracker, shared by all three jobs. Opens in
// place of the main content pane (never a modal takeover). The user can leave
// it — open a report from the sidebar — while the run keeps going; the footer
// returns them. Only the per-unit progress label differs per job kind:
//   report -> per-step, portfolio -> per-holding, trade -> per-cell.

const RUN_UNITS = {
  report: {
    title: "Generating this week's issue",
    unitLabel: "step",
    rows: [{
      name: "Ingest research inbox",
      status: "done"
    }, {
      name: "Head analyst · first pass",
      status: "done"
    }, {
      name: "Bull voice · stress-test",
      status: "done"
    }, {
      name: "Bear voice · stress-test",
      status: "running"
    }, {
      name: "Balanced voice · reconcile",
      status: "queued"
    }, {
      name: "Retrospective · grade issue 140",
      status: "queued"
    }, {
      name: "Synthesize · render Markdown",
      status: "queued"
    }]
  },
  portfolio: {
    title: "Analyzing portfolio · 23 holdings",
    unitLabel: "holding",
    rows: [{
      name: "NVDA · grade + targets",
      status: "done"
    }, {
      name: "ASML · grade + targets",
      status: "done"
    }, {
      name: "XOM · grade + targets",
      status: "done"
    }, {
      name: "VTI · reduced verdict",
      status: "running"
    }, {
      name: "RXRX · evidence check",
      status: "queued"
    }, {
      name: "Roll-up · construction panel",
      status: "queued"
    }]
  },
  trade: {
    title: "Discovering opportunities · 3 × 3 matrix",
    unitLabel: "cell",
    rows: [{
      name: "High · short",
      status: "done"
    }, {
      name: "High · mid",
      status: "done"
    }, {
      name: "High · long",
      status: "done"
    }, {
      name: "Medium · short",
      status: "running"
    }, {
      name: "Medium · mid",
      status: "queued"
    }, {
      name: "Low · short → long",
      status: "queued"
    }, {
      name: "Calibration scorecard",
      status: "queued"
    }]
  }
};
const STREAM_SAMPLE = {
  report: "…the Bear read cannot be fully refuted. Demand destruction is doing more work than the consensus expects; the 2014 analogue is more relevant than the 2007 one. We hold this next to the thesis as a permanent caveat rather than",
  portfolio: "…VTI graded on exposure, valuation, and house-view — no company-quality score applies to a broad index. Held as ballast at 16.2%, inside the 14–18% band. Action: hold. The reduced card is legitimate, not broken;",
  trade: "…Medium · short: VRT clears the gate on backlog inflection and the liquid-cooling attach rate. Conviction 4. Narrative and reality are converging mid-cycle. Bear: hyperscaler capex digestion pauses order flow. Entry matters more than"
};
function RunStatusPip({
  status
}) {
  const map = {
    done: {
      ch: "\u2713",
      color: "var(--ana-up)"
    },
    running: {
      ch: "\u25B8",
      color: "var(--accent)"
    },
    queued: {
      ch: "\u00B7",
      color: "var(--ink-3)"
    }
  };
  const m = map[status] || map.queued;
  return /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 12,
      color: m.color,
      width: 12,
      display: "inline-block",
      textAlign: "center"
    }
  }, m.ch);
}
function RunTracker({
  kind = "report",
  onLeave,
  onCancel
}) {
  const cfg = RUN_UNITS[kind] || RUN_UNITS.report;
  const doneCount = cfg.rows.filter(r => r.status === "done").length;
  const runningIdx = cfg.rows.findIndex(r => r.status === "running");
  const pct = Math.round((doneCount + 0.4) / cfg.rows.length * 100);
  return /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflowY: "auto",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      padding: "10px 32px",
      borderBottom: "1px solid var(--hairline)",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement(AnaHead, null, "Run tracker \xB7 ", cfg.unitLabel, " progress"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 8,
      alignItems: "center"
    }
  }, /*#__PURE__*/React.createElement("button", {
    onClick: onLeave,
    style: {
      padding: "6px 11px",
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      fontWeight: 500,
      whiteSpace: "nowrap",
      border: "1px solid var(--ink)",
      background: "transparent",
      color: "var(--ink)",
      cursor: "pointer",
      borderRadius: 2
    }
  }, "Leave \u2014 keeps running"), /*#__PURE__*/React.createElement("button", {
    onClick: onCancel,
    style: {
      padding: "6px 11px",
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      fontWeight: 500,
      whiteSpace: "nowrap",
      border: "1px solid var(--hairline)",
      background: "transparent",
      color: "var(--ink-2)",
      cursor: "pointer",
      borderRadius: 2
    }
  }, "Cancel run"))), /*#__PURE__*/React.createElement("div", {
    style: {
      maxWidth: 820,
      margin: "0 auto",
      padding: "32px 32px 96px"
    }
  }, /*#__PURE__*/React.createElement("h2", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 22,
      fontWeight: 600,
      color: "var(--ink)",
      margin: "0 0 4px"
    }
  }, cfg.title), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 14,
      fontStyle: "italic",
      color: "var(--ink-3)",
      margin: "0 0 4px"
    }
  }, "You can leave this view \u2014 open a report from the sidebar \u2014 and the run will keep going in the background."), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 14,
      margin: "20px 0 24px"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      height: 1,
      background: "var(--hairline-soft)",
      position: "relative",
      overflow: "hidden"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: "absolute",
      left: 0,
      top: 0,
      bottom: 0,
      width: pct + "%",
      background: "var(--ink)"
    }
  })), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 12,
      color: "var(--ink-3)",
      fontVariantNumeric: "tabular-nums"
    }
  }, doneCount, " / ", cfg.rows.length, " ", cfg.unitLabel, "s")), /*#__PURE__*/React.createElement(AnaCard, null, cfg.rows.map((r, i) => /*#__PURE__*/React.createElement("div", {
    key: r.name,
    style: {
      display: "flex",
      alignItems: "center",
      gap: 12,
      padding: "10px 16px",
      borderBottom: i < cfg.rows.length - 1 ? "1px solid var(--hairline-soft)" : "none",
      background: r.status === "running" ? "var(--paper-soft)" : "transparent"
    }
  }, /*#__PURE__*/React.createElement(RunStatusPip, {
    status: r.status
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      flex: 1,
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      color: r.status === "queued" ? "var(--ink-3)" : "var(--ink)",
      fontWeight: r.status === "running" ? 600 : 400
    }
  }, r.name), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)"
    }
  }, r.status)))), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 20
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      marginBottom: 8
    }
  }, "Streamed output \xB7 ", cfg.rows[runningIdx]?.name || "—"), /*#__PURE__*/React.createElement("div", {
    style: {
      border: "1px solid var(--hairline)",
      borderRadius: 2,
      padding: "14px 16px",
      background: "var(--paper-edge)",
      fontFamily: "var(--font-mono)",
      fontSize: 12,
      lineHeight: 1.6,
      color: "var(--ink-2)"
    }
  }, STREAM_SAMPLE[kind], /*#__PURE__*/React.createElement("span", {
    style: {
      display: "inline-block",
      width: 7,
      height: 14,
      background: "var(--ink-2)",
      marginLeft: 2,
      verticalAlign: "text-bottom"
    }
  })))));
}
Object.assign(window, {
  RunTracker
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/RunTracker.jsx", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/Settings.jsx
try { (() => {
// Settings.jsx — the tightest surface. Single-column form, label above
// field, no decorative grouping cards.

function Field({
  label,
  hint,
  children,
  mono
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      marginBottom: 28,
      maxWidth: 480
    }
  }, /*#__PURE__*/React.createElement("label", {
    style: {
      display: "block",
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)",
      marginBottom: 6
    }
  }, label), children, hint && /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-serif)",
      fontStyle: "italic",
      fontSize: 13,
      color: "var(--ink-3)",
      marginTop: 6,
      lineHeight: 1.45
    }
  }, hint));
}
function TextInput({
  value,
  onChange,
  placeholder,
  mono,
  type = "text"
}) {
  const [focus, setFocus] = React.useState(false);
  return /*#__PURE__*/React.createElement("input", {
    type: type,
    value: value,
    placeholder: placeholder,
    onChange: e => onChange?.(e.target.value),
    onFocus: () => setFocus(true),
    onBlur: () => setFocus(false),
    style: {
      display: "block",
      width: "100%",
      padding: "8px 0",
      background: "transparent",
      border: 0,
      borderBottom: "1px solid " + (focus ? "var(--accent)" : "var(--ink)"),
      boxShadow: focus ? "0 1px 0 0 var(--accent)" : "none",
      outline: "none",
      fontFamily: mono ? "var(--font-mono)" : "var(--font-sans)",
      fontVariantNumeric: mono ? "tabular-nums" : "normal",
      fontSize: 14,
      color: "var(--ink)",
      transition: "border-color 120ms cubic-bezier(0.4, 0.0, 0.2, 1)"
    }
  });
}
function RadioGroup({
  value,
  onChange,
  options
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 0
    }
  }, options.map(opt => /*#__PURE__*/React.createElement("label", {
    key: opt.value,
    style: {
      display: "flex",
      alignItems: "flex-start",
      gap: 10,
      padding: "10px 0",
      borderBottom: "1px solid var(--hairline-soft)",
      cursor: "pointer"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 14,
      height: 14,
      borderRadius: 7,
      marginTop: 3,
      border: "1px solid var(--ink)",
      background: "var(--paper)",
      position: "relative",
      flexShrink: 0
    }
  }, value === opt.value && /*#__PURE__*/React.createElement("span", {
    style: {
      position: "absolute",
      inset: 3,
      borderRadius: "50%",
      background: "var(--accent)"
    }
  })), /*#__PURE__*/React.createElement("span", null, /*#__PURE__*/React.createElement("span", {
    style: {
      display: "block",
      fontFamily: "var(--font-sans)",
      fontSize: 14,
      color: "var(--ink)",
      fontWeight: 500
    }
  }, opt.label), opt.hint && /*#__PURE__*/React.createElement("span", {
    style: {
      display: "block",
      marginTop: 2,
      fontFamily: "var(--font-serif)",
      fontStyle: "italic",
      fontSize: 13,
      color: "var(--ink-3)",
      lineHeight: 1.45
    }
  }, opt.hint)), /*#__PURE__*/React.createElement("input", {
    type: "radio",
    checked: value === opt.value,
    onChange: () => onChange(opt.value),
    style: {
      display: "none"
    }
  }))));
}
function Toggle({
  value,
  onChange
}) {
  // A boxy switch — no pill, no rounded slider. Just two states.
  return /*#__PURE__*/React.createElement("div", {
    onClick: () => onChange(!value),
    role: "switch",
    "aria-checked": value,
    style: {
      display: "inline-flex",
      alignItems: "center",
      gap: 2,
      padding: 2,
      width: 44,
      height: 22,
      border: "1px solid var(--ink)",
      borderRadius: 2,
      background: "transparent",
      cursor: "pointer",
      transition: "all 120ms cubic-bezier(0.4, 0.0, 0.2, 1)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 18,
      height: 16,
      borderRadius: 1,
      background: value ? "var(--ink)" : "transparent",
      marginLeft: value ? 20 : 0,
      transition: "margin-left 120ms cubic-bezier(0.4, 0.0, 0.2, 1)"
    }
  }));
}
function Settings() {
  const [provider, setProvider] = React.useState("anthropic");
  const [model, setModel] = React.useState("claude-opus-4-5");
  const [apiKey, setApiKey] = React.useState("sk-ant-•••• •••• •••• 92fa");
  const [folder, setFolder] = React.useState("/Users/desk/MarketSignal");
  const [autorun, setAutorun] = React.useState(true);
  const [dark, setDark] = React.useState(false);
  return /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflowY: "auto",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "10px 32px",
      borderBottom: "1px solid var(--hairline)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)"
    }
  }, "Settings")), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "40px 32px 96px 32px",
      maxWidth: 640
    }
  }, /*#__PURE__*/React.createElement("h1", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 28,
      lineHeight: 1.2,
      fontWeight: 600,
      color: "var(--ink)",
      margin: "0 0 6px 0"
    }
  }, "Settings"), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 15,
      lineHeight: 1.5,
      letterSpacing: "-0.006em",
      color: "var(--ink-2)",
      margin: "0 0 36px 0",
      maxWidth: "60ch",
      fontStyle: "italic"
    }
  }, "The reading surface is the product. These controls exist so it can run; they do not exist to be redesigned around."), /*#__PURE__*/React.createElement(Field, {
    label: "Model provider",
    hint: "Local-first. Keys are stored in your OS keychain. Nothing leaves your machine until you generate an issue."
  }, /*#__PURE__*/React.createElement(RadioGroup, {
    value: provider,
    onChange: setProvider,
    options: [{
      value: "anthropic",
      label: "Anthropic",
      hint: "Claude — used for the Head Analyst voice."
    }, {
      value: "openai",
      label: "OpenAI",
      hint: "GPT — alternate Head Analyst."
    }, {
      value: "local",
      label: "Local model (Ollama)",
      hint: "For users running their own inference."
    }]
  })), /*#__PURE__*/React.createElement(Field, {
    label: "Model",
    hint: "Used for the Head Market Analyst pass. Stress-test voices (Bull / Bear / Balanced) reuse the same credentials."
  }, /*#__PURE__*/React.createElement(TextInput, {
    value: model,
    onChange: setModel,
    mono: true
  })), /*#__PURE__*/React.createElement(Field, {
    label: "API key"
  }, /*#__PURE__*/React.createElement(TextInput, {
    value: apiKey,
    onChange: setApiKey,
    placeholder: "sk-...",
    mono: true
  })), /*#__PURE__*/React.createElement(Field, {
    label: "Issue storage folder",
    hint: "Issues are written as plain Markdown next to their figures. You can grep them."
  }, /*#__PURE__*/React.createElement(TextInput, {
    value: folder,
    onChange: setFolder,
    mono: true
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "flex-start",
      justifyContent: "space-between",
      gap: 24,
      padding: "16px 0",
      borderTop: "1px solid var(--hairline)",
      marginTop: 8
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      maxWidth: "44ch"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 14,
      color: "var(--ink)",
      fontWeight: 500,
      marginBottom: 2
    }
  }, "Generate Sunday issue automatically"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-serif)",
      fontStyle: "italic",
      fontSize: 13,
      color: "var(--ink-3)",
      lineHeight: 1.45
    }
  }, "Starts at 04:00 ET. The job takes about 30 minutes.")), /*#__PURE__*/React.createElement(Toggle, {
    value: autorun,
    onChange: setAutorun
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "flex-start",
      justifyContent: "space-between",
      gap: 24,
      padding: "16px 0",
      borderTop: "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      maxWidth: "44ch"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 14,
      color: "var(--ink)",
      fontWeight: 500,
      marginBottom: 2
    }
  }, "Dark surface"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-serif)",
      fontStyle: "italic",
      fontSize: 13,
      color: "var(--ink-3)",
      lineHeight: 1.45
    }
  }, "Warm graphite, never pure black.")), /*#__PURE__*/React.createElement(Toggle, {
    value: dark,
    onChange: setDark
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 40,
      display: "flex",
      gap: 10
    }
  }, /*#__PURE__*/React.createElement("button", {
    style: {
      padding: "9px 16px",
      background: "var(--ink)",
      color: "var(--paper)",
      border: "1px solid var(--ink)",
      borderRadius: 2,
      fontFamily: "var(--font-sans)",
      fontSize: 14,
      fontWeight: 500,
      cursor: "pointer"
    }
  }, "Save"), /*#__PURE__*/React.createElement("button", {
    style: {
      padding: "9px 16px",
      background: "transparent",
      color: "var(--ink)",
      border: "1px solid var(--ink)",
      borderRadius: 2,
      fontFamily: "var(--font-sans)",
      fontSize: 14,
      fontWeight: 500,
      cursor: "pointer"
    }
  }, "Test connection"))));
}
Object.assign(window, {
  Settings,
  Field,
  TextInput,
  RadioGroup,
  Toggle
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/Settings.jsx", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/Sidebar.jsx
try { (() => {
// Sidebar.jsx — the ONE shared-history sidebar. Same structure and treatment
// everywhere; only the content swaps per feature: recent report issues /
// recent Portfolio runs / recent Trade Opportunities runs. Same density, same
// selected-item accent (the oxblood leading-edge rule). A scoped extension of
// the report-history sidebar — not a new navigation pattern for the new pages.

function SidebarHeader({
  children
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.08em",
      textTransform: "uppercase",
      color: "var(--ink-3)",
      padding: "14px 20px 8px 20px"
    }
  }, children);
}
function ReportRow({
  report,
  isCurrent,
  isNew,
  onClick
}) {
  const [hover, setHover] = React.useState(false);
  return /*#__PURE__*/React.createElement("div", {
    onClick: onClick,
    onMouseEnter: () => setHover(true),
    onMouseLeave: () => setHover(false),
    style: {
      position: "relative",
      display: "grid",
      gridTemplateColumns: "1fr max-content",
      gap: 8,
      alignItems: "baseline",
      padding: "8px 16px 8px 18px",
      borderLeft: "2px solid " + (isCurrent ? "var(--accent)" : "transparent"),
      borderBottom: "1px solid var(--hairline-soft)",
      background: isCurrent || hover ? "var(--paper-soft)" : "transparent",
      cursor: "pointer",
      transition: "background-color 120ms cubic-bezier(0.4, 0.0, 0.2, 1)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      minWidth: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      fontWeight: isCurrent ? 600 : 500,
      color: "var(--ink)",
      overflow: "hidden",
      textOverflow: "ellipsis",
      whiteSpace: "nowrap"
    }
  }, report.title), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)",
      marginTop: 2
    }
  }, report.date, " \xB7 #", report.id, isNew ? /*#__PURE__*/React.createElement("span", {
    style: {
      marginLeft: 6,
      color: "var(--accent)"
    }
  }, "new") : null)), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontVariantNumeric: "tabular-nums",
      fontSize: 11,
      color: "var(--ink-3)"
    }
  }, report.read));
}

// Run row — same density/treatment, content swapped for Portfolio / TO runs.
function RunRow({
  run,
  isCurrent,
  onClick
}) {
  const [hover, setHover] = React.useState(false);
  return /*#__PURE__*/React.createElement("div", {
    onClick: onClick,
    onMouseEnter: () => setHover(true),
    onMouseLeave: () => setHover(false),
    style: {
      position: "relative",
      display: "grid",
      gridTemplateColumns: "1fr max-content",
      gap: 8,
      alignItems: "baseline",
      padding: "8px 16px 8px 18px",
      borderLeft: "2px solid " + (isCurrent ? "var(--accent)" : "transparent"),
      borderBottom: "1px solid var(--hairline-soft)",
      background: isCurrent || hover ? "var(--paper-soft)" : "transparent",
      cursor: "pointer",
      transition: "background-color 120ms cubic-bezier(0.4, 0.0, 0.2, 1)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      minWidth: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      fontWeight: isCurrent ? 600 : 500,
      color: "var(--ink)",
      overflow: "hidden",
      textOverflow: "ellipsis",
      whiteSpace: "nowrap"
    }
  }, run.label), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 10,
      letterSpacing: "0.04em",
      color: "var(--ink-3)",
      marginTop: 2,
      fontVariantNumeric: "tabular-nums"
    }
  }, run.date)), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-mono)",
      fontVariantNumeric: "tabular-nums",
      fontSize: 11,
      color: "var(--ink-3)"
    }
  }, run.read));
}
function NavItem({
  icon,
  label,
  badge,
  active,
  onClick
}) {
  const [hover, setHover] = React.useState(false);
  return /*#__PURE__*/React.createElement("div", {
    onClick: onClick,
    onMouseEnter: () => setHover(true),
    onMouseLeave: () => setHover(false),
    style: {
      display: "flex",
      alignItems: "center",
      gap: 10,
      padding: "8px 18px",
      borderLeft: "2px solid " + (active ? "var(--accent)" : "transparent"),
      background: active || hover ? "var(--paper-soft)" : "transparent",
      cursor: "pointer",
      transition: "background-color 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      color: "var(--ink)",
      fontWeight: active ? 600 : 500
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: icon,
    size: 14,
    color: "var(--ink-2)"
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      flex: 1
    }
  }, label), badge != null && /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontVariantNumeric: "tabular-nums",
      fontSize: 11,
      color: "var(--ink-3)"
    }
  }, badge));
}

// Maps a view to its owning feature (drives which history list shows).
function featureOf(view) {
  if (view === "portfolio") return "portfolio";
  if (view === "trade") return "trade";
  if (view === "report" || view === "runtracker") return "report";
  return "report"; // inbox / archive / settings keep the report list visible
}
function HistoryList({
  feature,
  currentReportId,
  setCurrentReportId,
  currentRunId,
  setCurrentRunId,
  setView
}) {
  const {
    RECENT_REPORTS,
    PORTFOLIO_RUNS,
    TO_RUNS
  } = window.MS_DATA;
  if (feature === "portfolio") {
    return /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(SidebarHeader, null, "Portfolio runs \xB7 recent"), /*#__PURE__*/React.createElement("div", {
      style: {
        flex: 1,
        overflowY: "auto",
        borderTop: "1px solid var(--hairline)"
      }
    }, PORTFOLIO_RUNS.map(r => /*#__PURE__*/React.createElement(RunRow, {
      key: r.id,
      run: r,
      isCurrent: r.id === currentRunId,
      onClick: () => {
        setView("portfolio");
        setCurrentRunId(r.id);
      }
    }))));
  }
  if (feature === "trade") {
    return /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(SidebarHeader, null, "Trade Opportunities runs \xB7 recent"), /*#__PURE__*/React.createElement("div", {
      style: {
        flex: 1,
        overflowY: "auto",
        borderTop: "1px solid var(--hairline)"
      }
    }, TO_RUNS.map(r => /*#__PURE__*/React.createElement(RunRow, {
      key: r.id,
      run: r,
      isCurrent: r.id === currentRunId,
      onClick: () => {
        setView("trade");
        setCurrentRunId(r.id);
      }
    }))));
  }
  return /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(SidebarHeader, null, "Recent reports \xB7 last 30"), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflowY: "auto",
      borderTop: "1px solid var(--hairline)"
    }
  }, RECENT_REPORTS.map(r => /*#__PURE__*/React.createElement(ReportRow, {
    key: r.id,
    report: r,
    isNew: r.isNew,
    isCurrent: r.id === currentReportId,
    onClick: () => {
      setView("report");
      setCurrentReportId(r.id);
    }
  }))));
}
function Sidebar({
  view,
  setView,
  currentReportId,
  setCurrentReportId,
  currentRunId,
  setCurrentRunId,
  feature: featureProp
}) {
  const feature = featureProp || featureOf(view);
  return /*#__PURE__*/React.createElement("aside", {
    style: {
      width: 280,
      flexShrink: 0,
      borderRight: "1px solid var(--hairline)",
      background: "var(--paper)",
      display: "flex",
      flexDirection: "column",
      minHeight: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      borderBottom: "1px solid var(--hairline)",
      paddingBottom: 4
    }
  }, /*#__PURE__*/React.createElement(SidebarHeader, null, "Market Signal"), /*#__PURE__*/React.createElement(NavItem, {
    icon: "report",
    label: "Weekly report",
    active: feature === "report",
    onClick: () => setView("report")
  }), /*#__PURE__*/React.createElement(NavItem, {
    icon: "rule",
    label: "Portfolio analysis",
    active: feature === "portfolio",
    onClick: () => setView("portfolio")
  }), /*#__PURE__*/React.createElement(NavItem, {
    icon: "search",
    label: "Trade opportunities",
    active: feature === "trade",
    onClick: () => setView("trade")
  })), /*#__PURE__*/React.createElement(HistoryList, {
    feature: feature,
    currentReportId: currentReportId,
    setCurrentReportId: setCurrentReportId,
    currentRunId: currentRunId,
    setCurrentRunId: setCurrentRunId,
    setView: setView
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      borderTop: "1px solid var(--hairline)",
      paddingTop: 4
    }
  }, /*#__PURE__*/React.createElement(NavItem, {
    icon: "inbox",
    label: "Research inbox",
    badge: "7",
    active: view === "inbox",
    onClick: () => setView("inbox")
  }), /*#__PURE__*/React.createElement(NavItem, {
    icon: "archive",
    label: "Archive",
    active: view === "archive",
    onClick: () => setView("archive")
  }), /*#__PURE__*/React.createElement(NavItem, {
    icon: "settings",
    label: "Settings",
    active: view === "settings",
    onClick: () => setView("settings")
  })));
}
Object.assign(window, {
  Sidebar,
  ReportRow,
  RunRow,
  NavItem,
  SidebarHeader,
  HistoryList,
  featureOf
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/Sidebar.jsx", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/TradeOpportunities.jsx
try { (() => {
// TradeOpportunities.jsx — the 3 x 3 risk x horizon matrix (analytical
// register). Opportunity cards lead with a directional thesis; the leading
// metric is the visual spine. Since-flagged perf carries the restrained
// sparkline. Empty cells are honest, never errors.

const ARCHETYPE_LABEL = {
  "secular-compounder": "Secular compounder",
  "ai-infra": "AI infra",
  "commodity-cyclical": "Commodity cyclical",
  "disruptor": "Disruptor",
  "quality-compounder": "Quality compounder"
};
const STATUS_META = {
  "new": {
    label: "New",
    color: "var(--accent)"
  },
  "still-valid": {
    label: "Still valid",
    color: "var(--ana-up)"
  },
  "played-out": {
    label: "Played out",
    color: "var(--ink-3)"
  },
  "invalidated": {
    label: "Invalidated",
    color: "var(--ana-down)"
  }
};
function StatusDot({
  status
}) {
  const m = STATUS_META[status] || STATUS_META.new;
  return /*#__PURE__*/React.createElement("span", {
    style: {
      display: "inline-flex",
      alignItems: "center",
      gap: 5
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 5,
      height: 5,
      borderRadius: "50%",
      background: m.color,
      flexShrink: 0
    }
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-2)",
      whiteSpace: "nowrap"
    }
  }, m.label));
}
function SinceFlagged({
  s
}) {
  if (!s) {
    return /*#__PURE__*/React.createElement("div", {
      style: {
        padding: "10px 14px",
        borderTop: "1px solid var(--hairline-soft)",
        background: "var(--paper-edge)"
      }
    }, /*#__PURE__*/React.createElement(AnaHead, {
      style: {
        color: "var(--ink-3)",
        marginBottom: 2
      }
    }, "Since flagged"), /*#__PURE__*/React.createElement("div", {
      style: {
        fontFamily: "var(--font-serif)",
        fontSize: 12,
        fontStyle: "italic",
        color: "var(--ink-3)"
      }
    }, "Debut \u2014 no track record yet."));
  }
  return /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "10px 14px",
      borderTop: "1px solid var(--hairline-soft)",
      background: "var(--paper-edge)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      gap: 10,
      marginBottom: 6
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      color: "var(--ink-3)"
    }
  }, "Since flagged \xB7 ", s.windows), /*#__PURE__*/React.createElement(Sparkline, {
    data: s.curve,
    dir: s.return.dir,
    w: 84,
    h: 24
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1fr 1fr",
      gap: "4px 12px"
    }
  }, /*#__PURE__*/React.createElement(SF, {
    k: "Return",
    v: /*#__PURE__*/React.createElement(Dir, {
      dir: s.return.dir,
      size: 12
    }, s.return.val)
  }), /*#__PURE__*/React.createElement(SF, {
    k: "vs sector",
    v: /*#__PURE__*/React.createElement(Dir, {
      dir: s.vsSector.dir,
      size: 12
    }, s.vsSector.val)
  }), /*#__PURE__*/React.createElement(SF, {
    k: "Max DD",
    v: /*#__PURE__*/React.createElement("span", {
      style: {
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        color: "var(--ink-2)"
      }
    }, s.drawdown)
  }), /*#__PURE__*/React.createElement(SF, {
    k: "Metric",
    v: /*#__PURE__*/React.createElement("span", {
      style: {
        fontFamily: "var(--font-sans)",
        fontSize: 11,
        color: s.continuation === "broken" ? "var(--ana-down)" : s.continuation === "watch" ? "var(--ink-2)" : "var(--ana-up)"
      }
    }, s.continuation)
  })));
}
function SF({
  k,
  v
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "space-between",
      alignItems: "baseline",
      gap: 8
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.04em",
      textTransform: "uppercase",
      color: "var(--ink-3)"
    }
  }, k), v);
}
function OpportunityCard({
  o
}) {
  return /*#__PURE__*/React.createElement(AnaCard, {
    style: {
      display: "flex",
      flexDirection: "column"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "12px 14px 10px",
      borderBottom: "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      gap: 8,
      marginBottom: 6
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontWeight: 500,
      fontSize: 15,
      letterSpacing: "0.02em",
      color: "var(--ink)"
    }
  }, o.ticker), /*#__PURE__*/React.createElement(StatusDot, {
    status: o.status
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 6,
      flexWrap: "wrap"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.04em",
      textTransform: "uppercase",
      color: "var(--ink-2)",
      border: "1px solid var(--hairline)",
      borderRadius: 2,
      padding: "1px 5px"
    }
  }, ARCHETYPE_LABEL[o.archetype]), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.04em",
      textTransform: "uppercase",
      color: "var(--ink-3)"
    }
  }, o.mode))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "10px 14px"
    }
  }, /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 13,
      lineHeight: 1.45,
      letterSpacing: "-0.006em",
      color: "var(--ink)",
      margin: 0
    }
  }, o.thesis)), /*#__PURE__*/React.createElement("div", {
    style: {
      margin: "0 14px 10px",
      padding: "8px 10px",
      border: "1px solid var(--hairline)",
      borderRadius: 2,
      background: "var(--paper-soft)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.04em",
      textTransform: "uppercase",
      color: "var(--ink-3)",
      lineHeight: 1.3
    }
  }, o.metric.label), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "baseline",
      gap: 8,
      marginTop: 6
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 22,
      fontWeight: 500,
      color: "var(--ink)",
      fontVariantNumeric: "tabular-nums",
      letterSpacing: "-0.01em"
    }
  }, o.metric.val), /*#__PURE__*/React.createElement(Dir, {
    dir: o.metric.trend,
    size: 11
  }))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "0 14px 10px"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 8,
      marginBottom: 8
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      color: "var(--ink-3)",
      whiteSpace: "nowrap",
      marginTop: 1
    }
  }, "Catalyst"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 12,
      lineHeight: 1.4,
      color: "var(--ink-2)"
    }
  }, o.catalyst)), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 8
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      color: "var(--ink-3)"
    }
  }, "Conviction"), /*#__PURE__*/React.createElement(Conviction, {
    value: o.conviction
  }))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "0 14px 10px"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      marginBottom: 6
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      color: "var(--ink-3)",
      marginBottom: 2
    }
  }, "Narrative vs reality"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 12,
      lineHeight: 1.4,
      color: "var(--ink-2)"
    }
  }, o.narrative)), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      color: "var(--ana-down)",
      marginBottom: 2
    }
  }, "Bear case"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 12,
      lineHeight: 1.4,
      color: "var(--ink-2)"
    }
  }, o.bear))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "0 14px 10px"
    }
  }, /*#__PURE__*/React.createElement(Reveal, {
    label: "Falsifiers \xB7 lineage \xB7 entry"
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 8
    }
  }, /*#__PURE__*/React.createElement(RevRow, {
    k: "Key falsifiers",
    v: o.falsifiers
  }), o.tech && /*#__PURE__*/React.createElement(RevRow, {
    k: "Technology read",
    v: o.tech
  }), /*#__PURE__*/React.createElement(RevRow, {
    k: "Entry",
    v: o.entry
  }), /*#__PURE__*/React.createElement(RevRow, {
    k: "Risk / forensic",
    v: o.flags
  }), /*#__PURE__*/React.createElement(RevRow, {
    k: "Lineage",
    v: "world-change → mechanism → node → metric",
    mono: true
  })))), /*#__PURE__*/React.createElement(SinceFlagged, {
    s: o.since
  }));
}
function RevRow({
  k,
  v,
  mono
}) {
  return /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      color: "var(--ink-3)",
      marginBottom: 2
    }
  }, k), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: mono ? "var(--font-mono)" : "var(--font-serif)",
      fontSize: mono ? 11 : 12,
      lineHeight: 1.4,
      color: "var(--ink-2)"
    }
  }, v));
}
function EmptyCell() {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      border: "1px dashed var(--hairline)",
      borderRadius: 2,
      minHeight: 88,
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      padding: 14
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 12,
      fontStyle: "italic",
      color: "var(--ink-3)",
      textAlign: "center"
    }
  }, "Nothing qualified this run."));
}
function CalibrationScorecard({
  c
}) {
  return /*#__PURE__*/React.createElement(AnaCard, {
    style: {
      marginTop: 24
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "12px 18px",
      borderBottom: "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 10
    }
  }, /*#__PURE__*/React.createElement(AnaHead, null, "Calibration scorecard"), c.shadow && /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-2)",
      border: "1px solid var(--hairline)",
      borderRadius: 2,
      padding: "1px 6px"
    }
  }, "Shadow \xB7 not yet steering"))), /*#__PURE__*/React.createElement(KeyFigureStrip, {
    items: [{
      label: "Picks",
      value: c.picks
    }, {
      label: "Matured",
      value: c.matured
    }, {
      label: "Hit rate",
      value: c.hitRate
    }, {
      label: "Avg return",
      value: /*#__PURE__*/React.createElement(Dir, {
        dir: c.avgReturn.dir,
        size: 16
      }, c.avgReturn.val)
    }, {
      label: "vs benchmark",
      value: /*#__PURE__*/React.createElement(Dir, {
        dir: c.vsBench.dir,
        size: 16
      }, c.vsBench.val)
    }],
    style: {
      border: 0,
      borderRadius: 0
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "12px 18px",
      borderTop: "1px solid var(--hairline-soft)"
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      marginBottom: 4
    }
  }, "Failure modes"), /*#__PURE__*/React.createElement("p", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 13,
      lineHeight: 1.5,
      color: "var(--ink-2)",
      margin: 0,
      maxWidth: "78ch"
    }
  }, c.failures)));
}
function TOToolbar() {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      padding: "10px 32px",
      borderBottom: "1px solid var(--hairline)",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement(AnaHead, null, "Trade opportunities"), /*#__PURE__*/React.createElement("button", {
    style: {
      display: "inline-flex",
      alignItems: "center",
      gap: 6,
      padding: "7px 12px",
      whiteSpace: "nowrap",
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      fontWeight: 500,
      border: "1px solid var(--ink)",
      background: "var(--ink)",
      color: "var(--paper)",
      cursor: "pointer",
      borderRadius: 2
    }
  }, "Run discovery"));
}
const RISK_ROWS = [{
  key: "high",
  label: "High risk"
}, {
  key: "medium",
  label: "Medium risk"
}, {
  key: "low",
  label: "Low risk"
}];
const HORIZONS = [{
  key: "short",
  label: "Short term"
}, {
  key: "mid",
  label: "Mid term"
}, {
  key: "long",
  label: "Long term"
}];
function TradeOpportunities() {
  const {
    MATRIX,
    OPP,
    CALIBRATION
  } = window.MS_DATA;
  const total = RISK_ROWS.reduce((a, r) => a + HORIZONS.reduce((b, h) => b + (MATRIX[r.key][h.key]?.length || 0), 0), 0);
  return /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflowY: "auto",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement(TOToolbar, null), /*#__PURE__*/React.createElement("div", {
    style: {
      maxWidth: 1100,
      margin: "0 auto",
      padding: "20px 28px 96px"
    }
  }, CALIBRATION.shadow && /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 12,
      alignItems: "baseline",
      padding: "10px 14px",
      border: "1px solid var(--hairline)",
      borderRadius: 2,
      marginBottom: 20,
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement(AnaHead, {
    style: {
      color: "var(--ink)",
      whiteSpace: "nowrap"
    }
  }, "Shadow run"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 13,
      fontStyle: "italic",
      lineHeight: 1.45,
      color: "var(--ink-2)"
    }
  }, "Early runs are calibration. The scorecard below is shown honestly but is not yet steering which ideas surface.")), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "96px repeat(3, 1fr)",
      gap: 12,
      marginBottom: 8,
      alignItems: "end"
    }
  }, /*#__PURE__*/React.createElement("div", null), HORIZONS.map(h => /*#__PURE__*/React.createElement(AnaHead, {
    key: h.key,
    style: {
      paddingLeft: 2
    }
  }, h.label))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 12
    }
  }, RISK_ROWS.map(r => /*#__PURE__*/React.createElement("div", {
    key: r.key,
    style: {
      display: "grid",
      gridTemplateColumns: "96px repeat(3, 1fr)",
      gap: 12,
      alignItems: "start"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      paddingTop: 4
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 13,
      fontWeight: 600,
      color: "var(--ink)",
      letterSpacing: "0.01em"
    }
  }, r.label), /*#__PURE__*/React.createElement("div", {
    style: {
      height: 2,
      width: 24,
      background: "var(--accent)",
      marginTop: 6,
      opacity: 0.85
    }
  })), HORIZONS.map(h => {
    const tickers = MATRIX[r.key][h.key] || [];
    return /*#__PURE__*/React.createElement("div", {
      key: h.key,
      style: {
        display: "flex",
        flexDirection: "column",
        gap: 12,
        minWidth: 0
      }
    }, tickers.length === 0 ? /*#__PURE__*/React.createElement(EmptyCell, null) : tickers.map(t => /*#__PURE__*/React.createElement(OpportunityCard, {
      key: t,
      o: OPP[t]
    })));
  })))), total === 0 && /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "40px 0",
      textAlign: "center",
      fontFamily: "var(--font-serif)",
      fontSize: 15,
      fontStyle: "italic",
      color: "var(--ink-3)"
    }
  }, "Nothing qualified this run. The gates held; that is a result, not a failure."), /*#__PURE__*/React.createElement(CalibrationScorecard, {
    c: CALIBRATION
  })));
}
Object.assign(window, {
  TradeOpportunities,
  OpportunityCard,
  CalibrationScorecard
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/TradeOpportunities.jsx", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/WarningBar.jsx
try { (() => {
// WarningBar.jsx — persistent warning area. Always visible. No icon, no
// color flag, no chrome. The words are the alert.

function WarningBar({
  children,
  tag = "Active warning"
}) {
  if (!children) return null;
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "baseline",
      gap: 14,
      padding: "10px 32px",
      borderBottom: "1px solid var(--hairline)",
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.08em",
      textTransform: "uppercase",
      color: "var(--ink)",
      fontWeight: 600,
      whiteSpace: "nowrap"
    }
  }, tag), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 14,
      lineHeight: 1.5,
      letterSpacing: "-0.006em",
      color: "var(--ink)",
      fontStyle: "italic"
    }
  }, children));
}
Object.assign(window, {
  WarningBar
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/WarningBar.jsx", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/Window.jsx
try { (() => {
// Window.jsx — A clean Tauri desktop frame.
// No glass, no blur, no oversized radius. Just a hairline, traffic-light
// dots, and the wordmark in the titlebar — the only chrome required.

function TrafficLights() {
  const dot = bg => /*#__PURE__*/React.createElement("div", {
    style: {
      width: 11,
      height: 11,
      borderRadius: "50%",
      background: bg,
      border: "0.5px solid rgba(0,0,0,0.08)"
    }
  });
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 7,
      alignItems: "center"
    }
  }, dot("#ff5f57"), dot("#febc2e"), dot("#28c840"));
}
function TitleBar() {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      height: 36,
      flexShrink: 0,
      display: "flex",
      alignItems: "center",
      gap: 16,
      padding: "0 12px",
      background: "var(--paper)",
      borderBottom: "1px solid var(--hairline)",
      WebkitAppRegion: "drag"
    }
  }, /*#__PURE__*/React.createElement(TrafficLights, null), /*#__PURE__*/React.createElement("div", {
    style: {
      position: "absolute",
      left: "50%",
      transform: "translateX(-50%)",
      display: "flex",
      alignItems: "baseline",
      gap: 8,
      whiteSpace: "nowrap"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-serif)",
      fontSize: 15,
      fontWeight: 600,
      color: "var(--ink)",
      letterSpacing: 0,
      lineHeight: 1
    }
  }, "Market Signal"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 10,
      letterSpacing: "0.18em",
      textTransform: "uppercase",
      color: "var(--ink-3)"
    }
  }, "desk \xB7 v0.4")));
}
function MarketSignalWindow({
  children,
  width = 1200,
  height = 780
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      width,
      height,
      background: "var(--paper)",
      border: "1px solid var(--hairline)",
      borderRadius: 6,
      overflow: "hidden",
      position: "relative",
      display: "flex",
      flexDirection: "column",
      // a single low-intensity outer shadow to lift the window off
      // the page in screenshots — not used on real surfaces in the app.
      boxShadow: "0 18px 60px rgba(31, 26, 20, 0.18), 0 0 0 1px rgba(31,26,20,0.04)"
    }
  }, /*#__PURE__*/React.createElement(TitleBar, null), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      display: "flex",
      minHeight: 0
    }
  }, children));
}
Object.assign(window, {
  MarketSignalWindow,
  TrafficLights,
  TitleBar
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/Window.jsx", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/app.jsx
try { (() => {
// app.jsx — top-level state + view router. Adds the analytical surfaces
// (Portfolio, Trade Opportunities) and the one shared, leaveable run tracker.

// ---- Job-status footer — the run lives here. Not a modal. ----
function JobFooter({
  job,
  feature,
  onStart,
  onView,
  onDismiss
}) {
  const wrap = {
    display: "flex",
    alignItems: "center",
    gap: 14,
    padding: "8px 32px",
    borderTop: "1px solid var(--hairline)",
    background: "var(--paper)"
  };
  const startLabel = {
    report: "Generate now",
    portfolio: "Run analysis",
    trade: "Run discovery"
  }[feature] || "Run";
  if (job.state === "running") {
    return /*#__PURE__*/React.createElement("div", {
      style: wrap
    }, /*#__PURE__*/React.createElement("div", {
      style: {
        fontFamily: "var(--font-sans)",
        fontSize: 12,
        color: "var(--ink-2)",
        whiteSpace: "nowrap"
      }
    }, JOB_TITLE[job.kind], " \xB7 running in background"), /*#__PURE__*/React.createElement("div", {
      style: {
        flex: 1,
        height: 1,
        background: "var(--hairline-soft)",
        position: "relative",
        overflow: "hidden"
      }
    }, /*#__PURE__*/React.createElement("div", {
      style: {
        position: "absolute",
        left: 0,
        top: 0,
        bottom: 0,
        width: "46%",
        background: "var(--ink)"
      }
    })), /*#__PURE__*/React.createElement("button", {
      onClick: onView,
      style: footerBtn(true)
    }, "View progress"));
  }
  if (job.state === "done") {
    return /*#__PURE__*/React.createElement("div", {
      style: {
        ...wrap,
        justifyContent: "space-between"
      }
    }, /*#__PURE__*/React.createElement("div", {
      style: {
        fontFamily: "var(--font-sans)",
        fontSize: 11,
        letterSpacing: "0.05em",
        textTransform: "uppercase",
        color: "var(--ink-3)",
        whiteSpace: "nowrap"
      }
    }, JOB_TITLE[job.kind], " \xB7 complete \xB7 trace kept for this session"), /*#__PURE__*/React.createElement("div", {
      style: {
        display: "flex",
        gap: 8
      }
    }, /*#__PURE__*/React.createElement("button", {
      onClick: onView,
      style: footerBtn(false)
    }, "Latest run log"), /*#__PURE__*/React.createElement("button", {
      onClick: onStart,
      style: footerBtn(true)
    }, startLabel)));
  }
  // idle
  return /*#__PURE__*/React.createElement("div", {
    style: {
      ...wrap,
      justifyContent: "space-between"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: "var(--font-sans)",
      fontSize: 11,
      letterSpacing: "0.05em",
      textTransform: "uppercase",
      color: "var(--ink-3)",
      whiteSpace: "nowrap"
    }
  }, "Idle \xB7 only one run at a time \xB7 last completed Apr 12"), /*#__PURE__*/React.createElement("button", {
    onClick: onStart,
    style: footerBtn(true)
  }, startLabel));
}
const JOB_TITLE = {
  report: "Weekly issue",
  portfolio: "Portfolio analysis",
  trade: "Trade discovery"
};
function footerBtn(primary) {
  return {
    display: "inline-flex",
    alignItems: "center",
    gap: 6,
    padding: "5px 11px",
    fontFamily: "var(--font-sans)",
    fontSize: 12,
    fontWeight: 500,
    whiteSpace: "nowrap",
    border: "1px solid " + (primary ? "var(--ink)" : "var(--hairline)"),
    background: primary ? "var(--ink)" : "transparent",
    color: primary ? "var(--paper)" : "var(--ink-2)",
    cursor: "pointer",
    borderRadius: 2,
    transition: "all 120ms cubic-bezier(0.4, 0.0, 0.2, 1)"
  };
}

// ---- Per-view warning content (same band treatment everywhere) ----
const WARNINGS = {
  report: {
    tag: "Active warning",
    text: "Last month's energy call (issue 140) was early. The underlying logic still holds; the timing was wrong. See the retrospective in §2."
  },
  portfolio: {
    tag: "Schwab · re-auth",
    text: "Your Schwab access token expires in 3 days. Re-authenticate before the next run — the analysis job is gated on a live connection."
  },
  trade: {
    tag: "Calibration",
    text: "Trade Opportunities is in shadow mode. The calibration scorecard is shown honestly but is not yet steering which ideas surface."
  }
};
function App() {
  const {
    RECENT_REPORTS
  } = window.MS_DATA;
  const [view, setView] = React.useState("portfolio");
  const [currentReportId, setCurrentReportId] = React.useState(142);
  const [currentRunId, setCurrentRunId] = React.useState("pf-0412");
  // one run at a time across the whole app
  const [job, setJob] = React.useState({
    kind: "report",
    state: "idle",
    returnTo: "report"
  });
  const report = RECENT_REPORTS.find(r => r.id === currentReportId) || RECENT_REPORTS[0];
  const effectiveView = view === "archive" ? "inbox" : view;
  const feature = window.featureOf(view === "runtracker" ? job.returnTo : view);
  const warning = WARNINGS[feature] || WARNINGS.report;
  const startRun = () => {
    const f = feature;
    setJob({
      kind: f,
      state: "running",
      returnTo: f === "report" ? "report" : f
    });
    setView("runtracker");
  };
  const leaveTracker = () => setView(job.returnTo);
  const cancelRun = () => {
    setJob(j => ({
      ...j,
      state: "idle"
    }));
    setView(job.returnTo);
  };
  const viewProgress = () => setView("runtracker");
  return /*#__PURE__*/React.createElement(MarketSignalWindow, {
    width: 1280,
    height: 840
  }, /*#__PURE__*/React.createElement(Sidebar, {
    view: view,
    setView: setView,
    feature: feature,
    currentReportId: currentReportId,
    setCurrentReportId: setCurrentReportId,
    currentRunId: currentRunId,
    setCurrentRunId: setCurrentRunId
  }), /*#__PURE__*/React.createElement("main", {
    style: {
      flex: 1,
      display: "flex",
      flexDirection: "column",
      minWidth: 0,
      background: "var(--paper)"
    }
  }, /*#__PURE__*/React.createElement(WarningBar, {
    tag: warning.tag
  }, warning.text), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      display: "flex",
      minHeight: 0
    }
  }, view === "runtracker" && /*#__PURE__*/React.createElement(RunTracker, {
    kind: job.kind,
    onLeave: leaveTracker,
    onCancel: cancelRun
  }), view !== "runtracker" && effectiveView === "report" && /*#__PURE__*/React.createElement(LatestReport, {
    report: report
  }), view !== "runtracker" && effectiveView === "portfolio" && /*#__PURE__*/React.createElement(Portfolio, null), view !== "runtracker" && effectiveView === "trade" && /*#__PURE__*/React.createElement(TradeOpportunities, null), view !== "runtracker" && effectiveView === "inbox" && /*#__PURE__*/React.createElement(ResearchInbox, null), view !== "runtracker" && effectiveView === "settings" && /*#__PURE__*/React.createElement(Settings, null)), /*#__PURE__*/React.createElement(JobFooter, {
    job: job,
    feature: feature,
    onStart: startRun,
    onView: viewProgress,
    onDismiss: () => setJob(j => ({
      ...j,
      state: "idle"
    }))
  })));
}
const root = ReactDOM.createRoot(document.getElementById("root"));
root.render(/*#__PURE__*/React.createElement(App, null));
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/app.jsx", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/data.js
try { (() => {
// Sample data for the UI kit. Voice modeled on the brief: declarative,
// specific, willing to say what isn't known. No emoji. No exclamation marks.

const RECENT_REPORTS = [{
  id: 144,
  title: "The Bond Vigilantes Return",
  date: "Sun · Apr 14",
  read: "14m",
  isNew: true
}, {
  id: 143,
  title: "Energy Re-rating, Round Two",
  date: "Sun · Apr 07",
  read: "21m"
}, {
  id: 142,
  title: "A Quiet Week, Honestly",
  date: "Sun · Mar 31",
  read: "9m",
  current: true
}, {
  id: 141,
  title: "Disinflation Without a Recession",
  date: "Sun · Mar 24",
  read: "18m"
}, {
  id: 140,
  title: "The Energy Re-rating Is Real",
  date: "Sun · Mar 17",
  read: "16m"
}, {
  id: 139,
  title: "Earnings Season, Quietly",
  date: "Sun · Mar 10",
  read: "11m"
}, {
  id: 138,
  title: "Powell's Conditional Pivot",
  date: "Sun · Mar 03",
  read: "19m"
}, {
  id: 137,
  title: "The Yen, Again",
  date: "Sun · Feb 25",
  read: "13m"
}, {
  id: 136,
  title: "What the Curve Is Saying",
  date: "Sun · Feb 18",
  read: "22m"
}, {
  id: 135,
  title: "Magnificent Seven, Less Magnificent",
  date: "Sun · Feb 11",
  read: "17m"
}, {
  id: 134,
  title: "China Tape Bombs",
  date: "Sun · Feb 04",
  read: "15m"
}, {
  id: 133,
  title: "Why We Were Wrong on Rates",
  date: "Sun · Jan 28",
  read: "20m"
}];
const WATCHLIST = [{
  name: "S&P 500",
  last: "4,392.18",
  wk: "+1.42%",
  ytd: "+8.1%"
}, {
  name: "WTI Crude",
  last: "73.46",
  wk: "−0.83%",
  ytd: "+11.2%"
}, {
  name: "US 10Y Yield",
  last: "4.31%",
  wk: "+0.06",
  ytd: "+0.32"
}, {
  name: "US 2Y Yield",
  last: "4.69%",
  wk: "+0.04",
  ytd: "+0.18"
}, {
  name: "DXY",
  last: "104.27",
  wk: "+0.21%",
  ytd: "+1.8%"
}, {
  name: "Gold",
  last: "2,318.40",
  wk: "+0.42%",
  ytd: "+12.4%"
}, {
  name: "BTC/USD",
  last: "67,140",
  wk: "−2.10%",
  ytd: "+58.6%"
}];
const INBOX_ITEMS = [{
  id: 7,
  title: "Q1 letters — value managers",
  source: "PDF · 12 files",
  added: "Apr 12",
  tag: "letters"
}, {
  id: 6,
  title: "BIS quarterly review",
  source: "PDF · 142 pp",
  added: "Apr 09",
  tag: "central-bank"
}, {
  id: 5,
  title: "Note: rate-vol vs equity-vol",
  source: "User note",
  added: "Apr 08",
  tag: "research"
}, {
  id: 4,
  title: "Powell — Senate testimony",
  source: "Transcript",
  added: "Apr 05",
  tag: "central-bank"
}, {
  id: 3,
  title: "10-K — selected energy names",
  source: "PDF · 8 files",
  added: "Apr 02",
  tag: "filings"
}, {
  id: 2,
  title: "China credit data, Mar",
  source: "PBoC release",
  added: "Apr 01",
  tag: "data"
}, {
  id: 1,
  title: "ECB minutes — March",
  source: "PDF · 38 pp",
  added: "Mar 28",
  tag: "central-bank"
}];
window.MS_DATA = {
  RECENT_REPORTS,
  WATCHLIST,
  INBOX_ITEMS
};
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/data.js", error: String((e && e.message) || e) }); }

// ui_kits/market_signal_desktop/data_analytical.js
try { (() => {
// data_analytical.js — sample data for the analytical register
// (Portfolio Analysis + Trade Opportunities). Voice per the brief:
// declarative, specific, willing to name uncertainty. No emoji.

/* ---- Shared history sidebar content (per-feature run lists) ---- */
const PORTFOLIO_RUNS = [{
  id: "pf-0412",
  label: "Full book · 23 holdings",
  date: "Apr 12 · 09:14",
  read: "rated 19",
  current: true
}, {
  id: "pf-0405",
  label: "Full book · 23 holdings",
  date: "Apr 05 · 09:02",
  read: "rated 19"
}, {
  id: "pf-0329",
  label: "Full book · 22 holdings",
  date: "Mar 29 · 08:51",
  read: "rated 18"
}, {
  id: "pf-0322",
  label: "Energy sleeve only",
  date: "Mar 22 · 18:30",
  read: "rated 4"
}, {
  id: "pf-0315",
  label: "Full book · 22 holdings",
  date: "Mar 15 · 09:08",
  read: "rated 18"
}, {
  id: "pf-0308",
  label: "Full book · 21 holdings",
  date: "Mar 08 · 09:00",
  read: "rated 17"
}];
const TO_RUNS = [{
  id: "to-0412",
  label: "Full matrix · 3 × 3",
  date: "Apr 12 · 10:40",
  read: "18 ideas",
  current: true
}, {
  id: "to-0405",
  label: "Full matrix · 3 × 3",
  date: "Apr 05 · 10:31",
  read: "15 ideas"
}, {
  id: "to-0329",
  label: "Full matrix · 3 × 3",
  date: "Mar 29 · 10:22",
  read: "11 ideas"
}, {
  id: "to-0322",
  label: "High-risk tier only",
  date: "Mar 22 · 19:05",
  read: "6 ideas"
}, {
  id: "to-0315",
  label: "Full matrix · 3 × 3",
  date: "Mar 15 · 10:18",
  read: "9 ideas"
}];

/* ---- Portfolio roll-up (whole book) ---- */
const BOOK = {
  value: "$1.84M",
  holdings: 23,
  rated: 19,
  notRated: 4,
  sinceRun: {
    dir: "up",
    val: "2.1%"
  },
  posture: "Defensive",
  cash: "Hold 6.8% dry. Fund energy adds by trimming the AI-infra cluster, not by raising new cash.",
  concentration: [{
    cluster: "AI infrastructure",
    weight: "22.4%",
    names: 4,
    beta: "0.38",
    delta: {
      dir: "up",
      val: "1.9"
    }
  }, {
    cluster: "Mega-cap platform",
    weight: "18.1%",
    names: 3,
    beta: "0.31",
    delta: {
      dir: "up",
      val: "0.7"
    }
  }, {
    cluster: "Energy · upstream",
    weight: "11.6%",
    names: 2,
    beta: "0.14",
    delta: {
      dir: "down",
      val: "2.3"
    }
  }, {
    cluster: "Rates-sensitive",
    weight: "9.2%",
    names: 3,
    beta: "0.09",
    delta: {
      dir: "flat",
      val: "0.0"
    }
  }, {
    cluster: "Cash & equivalents",
    weight: "6.8%",
    names: 1,
    beta: "—",
    delta: {
      dir: "flat",
      val: "0.0"
    }
  }],
  overlap: [{
    name: "Semiconductor capex",
    holdings: "ASML · NVDA · VTI (12% look-through)",
    note: "Single factor, three sleeves. The book is more concentrated than the position weights imply."
  }, {
    name: "Long-duration rates",
    holdings: "TLT · REIT sleeve",
    note: "Both express the same cut path. Sized as one bet, not two."
  }],
  closed: [{
    ticker: "PXD",
    note: "Exited Apr 03 on the energy de-rate. Acknowledged here, not silently dropped."
  }, {
    ticker: "SQ",
    note: "Closed Mar 28 — thesis invalidated, not trimmed."
  }],
  notRatedRisk: "AAPL Jun puts carry 2.1% of book at risk on a sharp drawdown. Unmodeled, but material to the roll-up."
};

/* ---- Holdings (classified by asset type; class always shown) ---- */
const HOLDINGS = [{
  ticker: "ASML",
  name: "ASML Holding",
  sector: "Semiconductors",
  klass: "stock",
  state: "rated",
  grade: "A−",
  unrealized: {
    dir: "up",
    val: "34.6%"
  },
  thesis: "The single supplier of EUV lithography, and therefore a toll on every leading-edge node the AI build-out requires. The moat is not the order book; it is that no second source exists, and none is being built. We hold this for the decade, not the quarter.",
  sub: {
    quality: "A",
    valuation: "C",
    momentum: "B",
    risk: "B"
  },
  conviction: 4,
  eom: "$198",
  eoy: "$235",
  standalone: "Own here",
  health: "Net cash; through-cycle margins intact.",
  horizon: {
    short: "Range-bound into the July print",
    mid: "Re-rating resumes as 2nm orders land",
    long: "Structural toll compounds"
  },
  action: "Trim",
  targetWeight: "6.0–7.0%",
  weight: "8.4%",
  adj: "−1.4% · −$26k",
  rationale: "An A-grade business held at an oversized weight. We are trimming the position, not the thesis — concentration, not conviction, is the constraint.",
  scenarios: [{
    k: "Bear",
    p: "20%",
    t: "$150",
    note: "China export curbs deepen"
  }, {
    k: "Base",
    p: "55%",
    t: "$210",
    note: "Orders normalize H2"
  }, {
    k: "Bull",
    p: "25%",
    t: "$280",
    note: "2nm pull-forward"
  }],
  changed: {
    intrinsic: "unchanged",
    action: "hold → trim",
    position: "unchanged"
  },
  curve: [26, 24, 25, 20, 21, 16, 14, 15, 9, 6],
  triggers: {
    add: "Pullback below $170 with order book intact",
    trim: "Weight > 8% or valuation grade to D",
    sell: "A credible second EUV source emerges"
  }
}, {
  ticker: "XOM",
  name: "Exxon Mobil",
  sector: "Energy · integrated",
  klass: "stock",
  state: "rated",
  grade: "B−",
  unrealized: {
    dir: "down",
    val: "4.2%"
  },
  thesis: "The capital-discipline thesis from issue 140, held at the position level. Capex restraint is binding across the majors; the marginal barrel is no longer being underwritten. The four-week tape is not validating the timing — we hold the thesis, we no longer hold the timing.",
  sub: {
    quality: "B",
    valuation: "B",
    momentum: "D",
    risk: "C"
  },
  conviction: 3,
  eom: "$112",
  eoy: "$128",
  standalone: "Own here",
  health: "Free-cash-flow positive at $70 WTI.",
  horizon: {
    short: "Soft; crude range-bound",
    mid: "Re-rating if discipline holds",
    long: "Structural under-supply"
  },
  action: "Add",
  targetWeight: "5.0–6.0%",
  weight: "4.1%",
  adj: "+1.2% · +$22k",
  rationale: "Below target weight and the structural case is intact. Fund the add by trimming ASML — same dollar, better risk-adjusted entry.",
  scenarios: [{
    k: "Bear",
    p: "30%",
    t: "$92",
    note: "Demand destruction"
  }, {
    k: "Base",
    p: "50%",
    t: "$118",
    note: "Discipline holds"
  }, {
    k: "Bull",
    p: "20%",
    t: "$140",
    note: "Supply shock"
  }],
  changed: {
    intrinsic: "momentum B → D",
    action: "hold → add",
    position: "unchanged"
  },
  deadMoney: "Forward case clears the hurdle by 140bps — not flagged dead, but thin.",
  curve: [12, 13, 11, 12, 10, 11, 9, 10, 9, 8],
  triggers: {
    add: "WTI two weekly closes below $70 with thesis intact",
    trim: "Capex discipline breaks at two majors",
    sell: "Sustained demand inflection"
  }
}, {
  ticker: "NVDA",
  name: "NVIDIA",
  sector: "Semiconductors",
  klass: "stock",
  state: "rated",
  grade: "A",
  unrealized: {
    dir: "up",
    val: "112.8%"
  },
  thesis: "The compute layer of the AI build-out. The question is no longer demand; it is whether the current margin structure is a peak or a plateau. We treat it as a plateau and size for the drawdown we cannot rule out.",
  sub: {
    quality: "A",
    valuation: "D",
    momentum: "A",
    risk: "C"
  },
  conviction: 4,
  eom: "$920",
  eoy: "$1,080",
  standalone: "Own here",
  health: "Pristine; the risk is multiple, not model.",
  horizon: {
    short: "Momentum intact",
    mid: "Margin normalization watched",
    long: "Compute toll compounds"
  },
  action: "Hold",
  targetWeight: "7.0–9.0%",
  weight: "8.9%",
  adj: "0.0% · in band",
  rationale: "At target weight with an A composite. No action — the valuation grade is the only thing staying our hand from adding.",
  scenarios: [{
    k: "Bear",
    p: "25%",
    t: "$640",
    note: "Margin re-rate"
  }, {
    k: "Base",
    p: "50%",
    t: "$980",
    note: "Plateau holds"
  }, {
    k: "Bull",
    p: "25%",
    t: "$1,300",
    note: "Inference TAM expands"
  }],
  changed: {
    intrinsic: "unchanged",
    action: "unchanged",
    position: "unchanged"
  },
  curve: [70, 78, 82, 90, 96, 104, 98, 106, 114, 120],
  triggers: {
    add: "Valuation grade recovers to C on a drawdown",
    trim: "Weight > 9% or momentum breaks",
    sell: "Margin structure confirms peak"
  }
}, {
  ticker: "VTI",
  name: "Vanguard Total Market",
  sector: "US equity · broad",
  klass: "etf",
  state: "rated-reduced",
  grade: "B",
  unrealized: {
    dir: "up",
    val: "9.1%"
  },
  thesis: "The book's beta anchor. Graded on exposure, valuation, and house-view — there is no company quality to score. Held as ballast, not as a call.",
  sub: {
    exposure: "B",
    valuation: "C",
    houseView: "B"
  },
  conviction: 3,
  eom: "—",
  eoy: "—",
  standalone: "Own here",
  health: "Diversified; the valuation read is index-level.",
  action: "Hold",
  targetWeight: "14.0–18.0%",
  weight: "16.2%",
  adj: "0.0% · in band",
  rationale: "Ballast at target weight. The reduced card is legitimate — an index fund has no company-quality score to compute, and that absence is shown, not faked.",
  changed: {
    intrinsic: "unchanged",
    action: "unchanged",
    position: "unchanged"
  }
}, {
  ticker: "AAPL 6/21 P",
  name: "AAPL Jun 21 $180 put",
  sector: "Options · hedge",
  klass: "option",
  state: "not-rated",
  reason: "Options are not modeled by the grading engine. Shown for completeness; its 2.1%-of-book tail risk is carried into the roll-up.",
  weight: "0.4%"
}, {
  ticker: "USD Cash",
  name: "Cash & sweep",
  sector: "Cash",
  klass: "cash",
  state: "not-rated",
  reason: "Cash is not graded. Tracked as deployable dry powder in the construction panel.",
  weight: "6.8%"
}, {
  ticker: "RXRX",
  name: "Recursion Pharma",
  sector: "Biotech · AI-enabled",
  klass: "stock",
  state: "insufficient",
  reason: "Insufficient evidence to grade. The model abstains rather than issue a low grade on a name it cannot underwrite — this is an explicit abstention, not an F.",
  weight: "1.1%"
}];

/* ---- Trade Opportunities · 3 × 3 risk × horizon matrix ---- */
// Each opportunity is keyed into a cell. Empty cells are honest.
const OPP = {
  CEG: {
    ticker: "CEG",
    archetype: "ai-infra",
    mode: "continuation",
    status: "still-valid",
    thesis: "Nuclear baseload is the only dispatchable power that clears the AI data-center load curve. Constellation owns the fleet; the PPAs are being signed now.",
    metric: {
      label: "Contracted TWh (fwd 24m)",
      val: "184",
      trend: "up"
    },
    catalyst: "Two hyperscaler PPAs expected before the July print.",
    conviction: 4,
    narrative: "Reality ahead of narrative — the contracts are signed, the multiple has not caught up.",
    bear: "Regulated-rate pushback caps the PPA premium.",
    falsifiers: "A PPA repriced below $80/MWh; a fleet outage > 30 days.",
    entry: "Scale in below $190; full size on a power-price pullback.",
    flags: "Concentration: single counterparty class (hyperscalers).",
    since: {
      return: {
        dir: "up",
        val: "31.4%"
      },
      vsSector: {
        dir: "up",
        val: "18.2%"
      },
      drawdown: "−9.1%",
      continuation: "intact",
      windows: "1m · 3m",
      curve: [10, 12, 11, 15, 18, 17, 22, 26, 24, 29]
    }
  },
  VRT: {
    ticker: "VRT",
    archetype: "ai-infra",
    mode: "continuation",
    status: "still-valid",
    thesis: "Thermal and power management is the bottleneck inside the rack. Vertiv sells the picks for the liquid-cooling transition.",
    metric: {
      label: "Backlog ($B)",
      val: "7.4",
      trend: "up"
    },
    catalyst: "Liquid-cooling attach rate inflecting with the GB200 ramp.",
    conviction: 4,
    narrative: "Narrative and reality converging; the re-rate is mid-cycle.",
    bear: "Hyperscaler capex digestion pauses the order flow.",
    falsifiers: "Two quarters of flat backlog; attach-rate guidance cut.",
    entry: "Wait for a capex-scare pullback; the entry matters more than the thesis here.",
    flags: "Forensic: aggressive backlog recognition — watch the cash conversion.",
    since: {
      return: {
        dir: "up",
        val: "12.7%"
      },
      vsSector: {
        dir: "down",
        val: "1.4%"
      },
      drawdown: "−14.2%",
      continuation: "intact",
      windows: "1m",
      curve: [20, 22, 19, 24, 21, 26, 23, 28, 25, 27]
    }
  },
  FSLR: {
    ticker: "FSLR",
    archetype: "secular-compounder",
    mode: "early",
    status: "new",
    thesis: "Domestic-content solar is being re-shored by policy and demand. First Solar's thin-film avoids the polysilicon supply chain entirely.",
    metric: {
      label: "Booked GW (2026+)",
      val: "61",
      trend: "up"
    },
    catalyst: "IRA domestic-content adder finalized in the next ruling.",
    conviction: 3,
    narrative: "Narrative lagging reality — the bookings are de-risked through 2026.",
    bear: "Policy reversal post-election guts the adder.",
    falsifiers: "Adder struck down; a major booking cancellation.",
    entry: "Starter here; add on the policy ruling.",
    flags: "Event-impact name — see technology read below.",
    tech: "Thin-film efficiency now within 2pts of crystalline; the cost-per-watt gap is the moat.",
    since: null // debut — no track record yet
  },
  EQT: {
    ticker: "EQT",
    archetype: "commodity-cyclical",
    mode: "early",
    status: "still-valid",
    thesis: "Natural gas is the bridge fuel the AI power build-out cannot avoid. EQT is the lowest-cost Appalachian producer.",
    metric: {
      label: "Free-cash breakeven",
      val: "$2.10",
      trend: "down"
    },
    catalyst: "LNG export capacity steps up through 2025.",
    conviction: 3,
    narrative: "Reality ahead — the breakeven keeps falling while the multiple sits at trough.",
    bear: "A warm winter floods storage and caps the strip.",
    falsifiers: "Breakeven rises two quarters; LNG schedule slips.",
    entry: "Scale on gas-price weakness, not strength.",
    flags: "Cyclical — size for the drawdown.",
    since: {
      return: {
        dir: "down",
        val: "6.3%"
      },
      vsSector: {
        dir: "down",
        val: "3.1%"
      },
      drawdown: "−18.4%",
      continuation: "watch",
      windows: "1m · 3m",
      curve: [18, 16, 17, 14, 15, 13, 14, 12, 13, 11]
    }
  },
  ANET: {
    ticker: "ANET",
    archetype: "quality-compounder",
    mode: "continuation",
    status: "still-valid",
    thesis: "The networking layer of the AI cluster. Arista's merchant-silicon model wins as scale-out fabric standardizes.",
    metric: {
      label: "AI cluster design wins",
      val: "11",
      trend: "up"
    },
    catalyst: "400G→800G transition with the next hyperscaler refresh.",
    conviction: 4,
    narrative: "Fairly priced for the base case; the optionality is unpriced.",
    bear: "White-box switching commoditizes the merchant model.",
    falsifiers: "Design-win count flattens; gross margin < 60%.",
    entry: "Quality at a fair price — accumulate, do not chase.",
    flags: "None material.",
    since: {
      return: {
        dir: "up",
        val: "8.9%"
      },
      vsSector: {
        dir: "up",
        val: "2.1%"
      },
      drawdown: "−7.8%",
      continuation: "intact",
      windows: "1m · 3m · 6m",
      curve: [14, 15, 14, 16, 17, 16, 18, 19, 18, 20]
    }
  },
  IONQ: {
    ticker: "IONQ",
    archetype: "disruptor",
    mode: "early",
    status: "played-out",
    thesis: "Trapped-ion quantum has a coherence-time edge. The commercial timeline, however, keeps slipping past the window we underwrite.",
    metric: {
      label: "Algorithmic qubits",
      val: "36",
      trend: "flat"
    },
    catalyst: "A roadmap milestone — repeatedly deferred.",
    conviction: 2,
    narrative: "Narrative far ahead of reality. We are flagging this as played-out, not adding.",
    bear: "Commercial revenue stays a rounding error through the horizon.",
    falsifiers: "A named enterprise contract at scale would re-open it.",
    entry: "No entry at present size; held on the watchlist only.",
    flags: "Forensic: revenue quality thin; insider selling.",
    since: {
      return: {
        dir: "down",
        val: "22.6%"
      },
      vsSector: {
        dir: "down",
        val: "29.4%"
      },
      drawdown: "−41.0%",
      continuation: "broken",
      windows: "1m · 3m · 6m · 12m",
      curve: [30, 28, 31, 24, 20, 22, 16, 14, 12, 9]
    }
  },
  WM: {
    ticker: "WM",
    archetype: "quality-compounder",
    mode: "continuation",
    status: "still-valid",
    thesis: "The toll road of waste. Landfill scarcity is a regulated moat; price escalators run ahead of cost inflation every year, regardless of the cycle.",
    metric: {
      label: "Core price / yield",
      val: "+6.1%",
      trend: "up"
    },
    catalyst: "Renewable-natural-gas plants step into the numbers through 2025.",
    conviction: 4,
    narrative: "Fairly priced for a compounder; the RNG optionality is the unpriced leg.",
    bear: "A recession softens volume faster than price can offset.",
    falsifiers: "Core price below CPI two quarters; an RNG plant impairment.",
    entry: "Boring on purpose. Accumulate on any market-wide drawdown.",
    flags: "None material. The lowest-beta idea in the matrix.",
    since: {
      return: {
        dir: "up",
        val: "4.2%"
      },
      vsSector: {
        dir: "up",
        val: "0.9%"
      },
      drawdown: "−3.4%",
      continuation: "intact",
      windows: "1m · 3m",
      curve: [10, 10, 11, 11, 12, 12, 13, 13, 14, 15]
    }
  }
};

// Cell layout: rows = risk (high/medium/low), cols = horizon (short/mid/long)
const MATRIX = {
  high: {
    short: ["IONQ"],
    mid: ["FSLR", "EQT"],
    long: ["CEG"]
  },
  medium: {
    short: ["VRT"],
    mid: ["ANET"],
    long: []
  },
  low: {
    short: [],
    mid: ["WM"],
    long: []
  }
};
// (Some cells deliberately empty — "nothing qualified," honest, not an error.)

const CALIBRATION = {
  shadow: true,
  picks: 47,
  matured: 19,
  hitRate: "58%",
  avgReturn: {
    dir: "up",
    val: "6.4%"
  },
  vsBench: {
    dir: "up",
    val: "2.1%"
  },
  failures: "Two-thirds of misses were timing, not thesis — the metric inflected later than the window allowed."
};
window.MS_DATA = Object.assign(window.MS_DATA || {}, {
  PORTFOLIO_RUNS,
  TO_RUNS,
  BOOK,
  HOLDINGS,
  OPP,
  MATRIX,
  CALIBRATION
});
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/market_signal_desktop/data_analytical.js", error: String((e && e.message) || e) }); }

__ds_ns.DirectionalValue = __ds_scope.DirectionalValue;

__ds_ns.GradeChip = __ds_scope.GradeChip;

__ds_ns.KeyFigureStrip = __ds_scope.KeyFigureStrip;

})();
