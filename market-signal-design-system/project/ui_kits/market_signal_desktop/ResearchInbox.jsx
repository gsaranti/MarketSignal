// ResearchInbox.jsx — user-supplied PDFs and notes, organized for later
// citation. Dense, single-column list. No bulk action chrome.
//
// ⚠ SUPERSEDED (2026-06-04 UX pass): serif "Filed research" title dropped (the
// toolbar eyebrow names it). The "Drop … into this folder" copy was reworded —
// there is no in-window drag-drop; "Add files…" reveals the inbox folder. See
// project/README.md §Per-surface titles and §c empty-state copy.

function InboxToolbar() {
  const [hover, setHover] = React.useState(null);
  return (
    <div style={{
      display: "flex", alignItems: "center", justifyContent: "space-between",
      padding: "10px 32px",
      borderBottom: "1px solid var(--hairline)",
      background: "var(--paper)",
    }}>
      <div style={{
        fontFamily: "var(--font-sans)",
        fontSize: 11, letterSpacing: "0.05em", textTransform: "uppercase",
        color: "var(--ink-3)",
      }}>Research inbox</div>
      <div style={{ display: "flex", gap: 8 }}>
        <button
          onMouseEnter={() => setHover("add")}
          onMouseLeave={() => setHover(null)}
          style={{
            display: "inline-flex", alignItems: "center", gap: 6,
            padding: "7px 12px",
            fontFamily: "var(--font-sans)",
            fontSize: 13, fontWeight: 500,
            whiteSpace: "nowrap",
            border: "1px solid var(--ink)",
            background: hover === "add" ? "#2B241B" : "var(--ink)",
            color: "var(--paper)",
            cursor: "pointer", borderRadius: 2,
            transition: "all 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
          }}>
          <Icon name="plus" size={13} />
          Add file or note
        </button>
      </div>
    </div>
  );
}

function InboxRow({ item }) {
  const [hover, setHover] = React.useState(false);
  return (
    <div
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: "grid",
        gridTemplateColumns: "20px 1fr max-content max-content",
        gap: 14, alignItems: "baseline",
        padding: "12px 32px",
        borderBottom: "1px solid var(--hairline-soft)",
        background: hover ? "var(--paper-soft)" : "transparent",
        cursor: "pointer",
        transition: "background-color 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
      }}>
      <Icon name="file" size={14} color="var(--ink-2)" />
      <div>
        <div style={{
          fontFamily: "var(--font-sans)",
          fontSize: 14, color: "var(--ink)", fontWeight: 500,
        }}>{item.title}</div>
        <div style={{
          fontFamily: "var(--font-sans)",
          fontSize: 11, letterSpacing: "0.05em", textTransform: "uppercase",
          color: "var(--ink-3)", marginTop: 2,
        }}>{item.source} · {item.tag}</div>
      </div>
      <div style={{
        fontFamily: "var(--font-mono)",
        fontVariantNumeric: "tabular-nums",
        fontSize: 12, color: "var(--ink-3)",
      }}>added {item.added}</div>
      <div style={{
        fontFamily: "var(--font-mono)",
        fontVariantNumeric: "tabular-nums",
        fontSize: 12, color: "var(--ink-3)",
      }}>#{item.id}</div>
    </div>
  );
}

function ResearchInbox() {
  const { INBOX_ITEMS } = window.MS_DATA;
  return (
    <div style={{ flex: 1, overflowY: "auto", background: "var(--paper)" }}>
      <InboxToolbar />
      <div style={{ padding: "28px 32px 16px 32px", maxWidth: 920 }}>
        <h2 style={{
          fontFamily: "var(--font-serif)",
          fontSize: 22, lineHeight: 1.3, fontWeight: 600,
          color: "var(--ink)", margin: "0 0 6px 0",
        }}>Filed research</h2>
        <p style={{
          fontFamily: "var(--font-serif)",
          fontSize: 15, lineHeight: 1.5, letterSpacing: "-0.006em",
          color: "var(--ink-2)", margin: "0 0 20px 0",
          maxWidth: "62ch",
        }}>
          Drop PDFs, transcripts, or text notes into this folder and the
          analyst pipeline will consider them when writing next week's
          issue. Nothing is sent to a third party until you generate.
        </p>
      </div>
      <div style={{
        borderTop: "1px solid var(--hairline)",
        borderBottom: "1px solid var(--hairline)",
      }}>
        {INBOX_ITEMS.map(item => <InboxRow key={item.id} item={item} />)}
      </div>
      <div style={{
        padding: "16px 32px",
        fontFamily: "var(--font-sans)",
        fontSize: 11, letterSpacing: "0.05em", textTransform: "uppercase",
        color: "var(--ink-3)",
      }}>
        {INBOX_ITEMS.length} items · all local · last sync —
      </div>
    </div>
  );
}

Object.assign(window, { ResearchInbox, InboxRow });
