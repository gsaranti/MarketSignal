// Icon.jsx — outlined, single-weight (1.25px at 20px), squared caps.
// One stroke, one color. No fills, no two-tone, no brand-color interiors.

function Icon({ name, size = 16, color = "currentColor", style }) {
  const sw = size <= 14 ? 1.1 : 1.25;
  const v = 20; // viewBox
  const common = {
    width: size, height: size, viewBox: `0 0 ${v} ${v}`,
    fill: "none", stroke: color, strokeWidth: sw,
    strokeLinecap: "square", strokeLinejoin: "miter",
    style: { display: "block", flexShrink: 0, ...style },
  };
  const paths = {
    report:    <><rect x="4" y="3" width="12" height="14" /><line x1="6.5" y1="7" x2="13.5" y2="7" /><line x1="6.5" y1="10" x2="13.5" y2="10" /><line x1="6.5" y1="13" x2="11.5" y2="13" /></>,
    archive:   <><rect x="3" y="3" width="14" height="3" /><rect x="4" y="6" width="12" height="11" /><line x1="8" y1="10" x2="12" y2="10" /></>,
    inbox:     <><path d="M3 11l3 -7h8l3 7v6H3z" /><path d="M3 11h4l1 2h4l1 -2h4" /></>,
    settings:  <><circle cx="10" cy="10" r="2.2" /><path d="M10 3v2 M10 15v2 M3 10h2 M15 10h2 M5 5l1.4 1.4 M13.6 13.6l1.4 1.4 M5 15l1.4 -1.4 M13.6 6.4l1.4 -1.4" /></>,
    warning:   <><path d="M10 4l7 12H3z" /><line x1="10" y1="9" x2="10" y2="13" /><line x1="10" y1="14.5" x2="10" y2="14.6" /></>,
    check:     <><path d="M4 11l4 4 8 -10" /></>,
    chevron_r: <><path d="M8 5l5 5 -5 5" /></>,
    chevron_d: <><path d="M5 8l5 5 5 -5" /></>,
    search:    <><circle cx="8.5" cy="8.5" r="4.5" /><line x1="12" y1="12" x2="16" y2="16" /></>,
    plus:      <><line x1="10" y1="4" x2="10" y2="16" /><line x1="4" y1="10" x2="16" y2="10" /></>,
    export_:   <><path d="M10 13V3 M6 7l4 -4 4 4" /><path d="M3 13v4h14v-4" /></>,
    sidebar:   <><rect x="3" y="4" width="14" height="12" /><line x1="8" y1="4" x2="8" y2="16" /></>,
    rule:      <><line x1="3" y1="10" x2="17" y2="10" /></>,
    file:      <><path d="M5 3h7l3 3v11H5z" /><path d="M12 3v3h3" /></>,
    quote:     <><path d="M5 6h4v4H5z M5 10v3a2 2 0 002 2" /><path d="M11 6h4v4h-4z M11 10v3a2 2 0 002 2" /></>,
  };
  return <svg {...common}>{paths[name] || null}</svg>;
}

window.Icon = Icon;
