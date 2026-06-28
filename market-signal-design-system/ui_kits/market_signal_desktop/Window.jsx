// Window.jsx — A clean Tauri desktop frame.
// No glass, no blur, no oversized radius. Just a hairline, traffic-light
// dots, and the wordmark in the titlebar — the only chrome required.

function TrafficLights() {
  const dot = (bg) => (
    <div style={{
      width: 11, height: 11, borderRadius: "50%",
      background: bg, border: "0.5px solid rgba(0,0,0,0.08)",
    }} />
  );
  return (
    <div style={{ display: "flex", gap: 7, alignItems: "center" }}>
      {dot("#ff5f57")}{dot("#febc2e")}{dot("#28c840")}
    </div>
  );
}

function TitleBar() {
  return (
    <div style={{
      height: 36, flexShrink: 0,
      display: "flex", alignItems: "center", gap: 16,
      padding: "0 12px",
      background: "var(--paper)",
      borderBottom: "1px solid var(--hairline)",
      WebkitAppRegion: "drag",
    }}>
      <TrafficLights />
      <div style={{
        position: "absolute", left: "50%", transform: "translateX(-50%)",
        display: "flex", alignItems: "baseline", gap: 8,
        whiteSpace: "nowrap",
      }}>
        <span style={{
          fontFamily: "var(--font-serif)",
          fontSize: 15, fontWeight: 600, color: "var(--ink)",
          letterSpacing: 0, lineHeight: 1,
        }}>Market Signal</span>
        <span style={{
          fontFamily: "var(--font-sans)",
          fontSize: 10, letterSpacing: "0.18em", textTransform: "uppercase",
          color: "var(--ink-3)",
        }}>desk · v0.4</span>
      </div>
    </div>
  );
}

function MarketSignalWindow({ children, width = 1200, height = 780 }) {
  return (
    <div style={{
      width, height,
      background: "var(--paper)",
      border: "1px solid var(--hairline)",
      borderRadius: 6,
      overflow: "hidden",
      position: "relative",
      display: "flex", flexDirection: "column",
      // a single low-intensity outer shadow to lift the window off
      // the page in screenshots — not used on real surfaces in the app.
      boxShadow: "0 18px 60px rgba(31, 26, 20, 0.18), 0 0 0 1px rgba(31,26,20,0.04)",
    }}>
      <TitleBar />
      <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
        {children}
      </div>
    </div>
  );
}

Object.assign(window, { MarketSignalWindow, TrafficLights, TitleBar });
