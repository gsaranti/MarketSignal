// Settings.jsx — the tightest surface. Single-column form, label above
// field, no decorative grouping cards.

function Field({ label, hint, children, mono }) {
  return (
    <div style={{ marginBottom: 28, maxWidth: 480 }}>
      <label style={{
        display: "block",
        fontFamily: "var(--font-sans)",
        fontSize: 11, letterSpacing: "0.05em", textTransform: "uppercase",
        color: "var(--ink-3)",
        marginBottom: 6,
      }}>{label}</label>
      {children}
      {hint && (
        <div style={{
          fontFamily: "var(--font-serif)",
          fontStyle: "italic", fontSize: 13, color: "var(--ink-3)",
          marginTop: 6, lineHeight: 1.45,
        }}>{hint}</div>
      )}
    </div>
  );
}

function TextInput({ value, onChange, placeholder, mono, type = "text" }) {
  const [focus, setFocus] = React.useState(false);
  return (
    <input
      type={type}
      value={value}
      placeholder={placeholder}
      onChange={(e) => onChange?.(e.target.value)}
      onFocus={() => setFocus(true)}
      onBlur={() => setFocus(false)}
      style={{
        display: "block", width: "100%", padding: "8px 0",
        background: "transparent", border: 0,
        borderBottom: "1px solid " + (focus ? "var(--accent)" : "var(--ink)"),
        boxShadow: focus ? "0 1px 0 0 var(--accent)" : "none",
        outline: "none",
        fontFamily: mono ? "var(--font-mono)" : "var(--font-sans)",
        fontVariantNumeric: mono ? "tabular-nums" : "normal",
        fontSize: 14, color: "var(--ink)",
        transition: "border-color 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
      }}
    />
  );
}

function RadioGroup({ value, onChange, options }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 0 }}>
      {options.map(opt => (
        <label key={opt.value} style={{
          display: "flex", alignItems: "flex-start", gap: 10,
          padding: "10px 0",
          borderBottom: "1px solid var(--hairline-soft)",
          cursor: "pointer",
        }}>
          <span style={{
            width: 14, height: 14, borderRadius: 7, marginTop: 3,
            border: "1px solid var(--ink)",
            background: "var(--paper)",
            position: "relative", flexShrink: 0,
          }}>
            {value === opt.value && (
              <span style={{
                position: "absolute", inset: 3, borderRadius: "50%",
                background: "var(--accent)",
              }} />
            )}
          </span>
          <span>
            <span style={{
              display: "block",
              fontFamily: "var(--font-sans)",
              fontSize: 14, color: "var(--ink)",
              fontWeight: 500,
            }}>{opt.label}</span>
            {opt.hint && (
              <span style={{
                display: "block", marginTop: 2,
                fontFamily: "var(--font-serif)",
                fontStyle: "italic", fontSize: 13, color: "var(--ink-3)",
                lineHeight: 1.45,
              }}>{opt.hint}</span>
            )}
          </span>
          <input type="radio" checked={value === opt.value}
            onChange={() => onChange(opt.value)}
            style={{ display: "none" }} />
        </label>
      ))}
    </div>
  );
}

function Toggle({ value, onChange }) {
  // A boxy switch — no pill, no rounded slider. Just two states.
  return (
    <div
      onClick={() => onChange(!value)}
      role="switch" aria-checked={value}
      style={{
        display: "inline-flex", alignItems: "center",
        gap: 2, padding: 2,
        width: 44, height: 22,
        border: "1px solid var(--ink)", borderRadius: 2,
        background: "transparent", cursor: "pointer",
        transition: "all 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
      }}>
      <div style={{
        width: 18, height: 16, borderRadius: 1,
        background: value ? "var(--ink)" : "transparent",
        marginLeft: value ? 20 : 0,
        transition: "margin-left 120ms cubic-bezier(0.4, 0.0, 0.2, 1)",
      }} />
    </div>
  );
}

function Settings() {
  const [provider, setProvider] = React.useState("anthropic");
  const [model, setModel] = React.useState("claude-opus-4-5");
  const [apiKey, setApiKey] = React.useState("sk-ant-•••• •••• •••• 92fa");
  const [folder, setFolder] = React.useState("/Users/desk/MarketSignal");
  const [autorun, setAutorun] = React.useState(true);
  const [dark, setDark] = React.useState(false);

  return (
    <div style={{ flex: 1, overflowY: "auto", background: "var(--paper)" }}>
      <div style={{
        padding: "10px 32px",
        borderBottom: "1px solid var(--hairline)",
      }}>
        <div style={{
          fontFamily: "var(--font-sans)",
          fontSize: 11, letterSpacing: "0.05em", textTransform: "uppercase",
          color: "var(--ink-3)",
        }}>Settings</div>
      </div>

      <div style={{ padding: "40px 32px 96px 32px", maxWidth: 640 }}>
        <h1 style={{
          fontFamily: "var(--font-serif)",
          fontSize: 28, lineHeight: 1.2, fontWeight: 600,
          color: "var(--ink)", margin: "0 0 6px 0",
        }}>Settings</h1>
        <p style={{
          fontFamily: "var(--font-serif)",
          fontSize: 15, lineHeight: 1.5, letterSpacing: "-0.006em",
          color: "var(--ink-2)", margin: "0 0 36px 0",
          maxWidth: "60ch", fontStyle: "italic",
        }}>
          The reading surface is the product. These controls exist so it
          can run; they do not exist to be redesigned around.
        </p>

        <Field label="Model provider"
          hint="Local-first. Keys are stored in your OS keychain. Nothing leaves your machine until you generate an issue.">
          <RadioGroup value={provider} onChange={setProvider} options={[
            { value: "anthropic", label: "Anthropic",                 hint: "Claude — used for the Head Analyst voice." },
            { value: "openai",    label: "OpenAI",                     hint: "GPT — alternate Head Analyst." },
            { value: "local",     label: "Local model (Ollama)",       hint: "For users running their own inference." },
          ]} />
        </Field>

        <Field label="Model"
          hint="Used for the Head Market Analyst pass. Stress-test voices (Bull / Bear / Balanced) reuse the same credentials.">
          <TextInput value={model} onChange={setModel} mono />
        </Field>

        <Field label="API key">
          <TextInput value={apiKey} onChange={setApiKey} placeholder="sk-..." mono />
        </Field>

        <Field label="Issue storage folder"
          hint="Issues are written as plain Markdown next to their figures. You can grep them.">
          <TextInput value={folder} onChange={setFolder} mono />
        </Field>

        <div style={{
          display: "flex", alignItems: "flex-start",
          justifyContent: "space-between", gap: 24,
          padding: "16px 0",
          borderTop: "1px solid var(--hairline)",
          marginTop: 8,
        }}>
          <div style={{ maxWidth: "44ch" }}>
            <div style={{
              fontFamily: "var(--font-sans)",
              fontSize: 14, color: "var(--ink)", fontWeight: 500,
              marginBottom: 2,
            }}>Generate Sunday issue automatically</div>
            <div style={{
              fontFamily: "var(--font-serif)",
              fontStyle: "italic", fontSize: 13, color: "var(--ink-3)",
              lineHeight: 1.45,
            }}>Starts at 04:00 ET. The job takes about 30 minutes.</div>
          </div>
          <Toggle value={autorun} onChange={setAutorun} />
        </div>

        <div style={{
          display: "flex", alignItems: "flex-start",
          justifyContent: "space-between", gap: 24,
          padding: "16px 0",
          borderTop: "1px solid var(--hairline-soft)",
        }}>
          <div style={{ maxWidth: "44ch" }}>
            <div style={{
              fontFamily: "var(--font-sans)",
              fontSize: 14, color: "var(--ink)", fontWeight: 500,
              marginBottom: 2,
            }}>Dark surface</div>
            <div style={{
              fontFamily: "var(--font-serif)",
              fontStyle: "italic", fontSize: 13, color: "var(--ink-3)",
              lineHeight: 1.45,
            }}>Warm graphite, never pure black.</div>
          </div>
          <Toggle value={dark} onChange={setDark} />
        </div>

        <div style={{ marginTop: 40, display: "flex", gap: 10 }}>
          <button style={{
            padding: "9px 16px",
            background: "var(--ink)", color: "var(--paper)",
            border: "1px solid var(--ink)", borderRadius: 2,
            fontFamily: "var(--font-sans)", fontSize: 14, fontWeight: 500,
            cursor: "pointer",
          }}>Save</button>
          <button style={{
            padding: "9px 16px",
            background: "transparent", color: "var(--ink)",
            border: "1px solid var(--ink)", borderRadius: 2,
            fontFamily: "var(--font-sans)", fontSize: 14, fontWeight: 500,
            cursor: "pointer",
          }}>Test connection</button>
        </div>
      </div>
    </div>
  );
}

Object.assign(window, { Settings, Field, TextInput, RadioGroup, Toggle });
