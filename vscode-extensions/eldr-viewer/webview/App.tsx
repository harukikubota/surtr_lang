import React, { useState } from "react";

import type { ViewerFile } from "../src/viewerTypes";

type Props = {
  viewer: ViewerFile;
};

export function App({ viewer }: Props): JSX.Element {
  const [selectedFunctionId, setSelectedFunctionId] = useState<string | null>(
    viewer.functions[0]?.function_id ?? null
  );

  const selectedFunction =
    viewer.functions.find((item) => item.function_id === selectedFunctionId) ?? viewer.functions[0];
  const selectedSource =
    viewer.sources.find((item) => item.source_id === selectedFunction?.source_ref?.source_id) ??
    viewer.sources[0];

  return (
    <main style={styles.page}>
      <header style={styles.header}>
        <div>
          <h1 style={styles.title}>Eldr Viewer</h1>
          <p style={styles.subtitle}>
            {viewer.header.magic} v{viewer.header.version} / schema {viewer.schema_version}
          </p>
        </div>
      </header>

      <section style={styles.grid}>
        <aside style={styles.card}>
          <h2 style={styles.sectionTitle}>Functions</h2>
          {viewer.functions.map((item) => (
            <button
              key={item.function_id}
              type="button"
              style={{
                ...styles.functionButton,
                ...(item.function_id === selectedFunctionId ? styles.functionButtonActive : {})
              }}
              onClick={() => setSelectedFunctionId(item.function_id)}
            >
              {item.name ?? item.function_id}
            </button>
          ))}
        </aside>

        <section style={styles.card}>
          <h2 style={styles.sectionTitle}>Opcodes</h2>
          <div style={styles.list}>
            {viewer.opcodes
              .filter((row) => !selectedFunction || row.function_id === selectedFunction.function_id)
              .map((row) => (
                <div key={row.pc} style={styles.row}>
                  <strong>pc {row.pc}</strong>
                  <span>{row.op.kind}</span>
                  {row.label ? <span>{row.label}</span> : null}
                </div>
              ))}
          </div>
        </section>

        <section style={styles.card}>
          <h2 style={styles.sectionTitle}>Constants</h2>
          <div style={styles.list}>
            {viewer.constants.map((constant) => (
              <div key={`${constant.kind}-${constant.idx}`} style={styles.row}>
                <strong>{constant.kind}</strong>
                <span>{"value" in constant ? String(constant.value) : "()"}</span>
              </div>
            ))}
          </div>
        </section>

        <section style={styles.card}>
          <h2 style={styles.sectionTitle}>Source</h2>
          <pre style={styles.pre}>{selectedSource?.text ?? "No source text embedded."}</pre>
        </section>
      </section>
    </main>
  );
}

const styles: Record<string, React.CSSProperties> = {
  page: {
    fontFamily: '"Iosevka Curly", "SF Mono", Consolas, monospace',
    color: "#f6f1e8",
    background:
      "radial-gradient(circle at top left, rgba(228,122,70,0.25), transparent 30%), #171310",
    minHeight: "100vh",
    margin: 0,
    padding: "24px"
  },
  header: {
    marginBottom: "20px"
  },
  title: {
    margin: 0,
    fontSize: "28px"
  },
  subtitle: {
    margin: "6px 0 0",
    color: "#d4c7b7"
  },
  grid: {
    display: "grid",
    gridTemplateColumns: "minmax(180px, 220px) minmax(260px, 1fr) minmax(220px, 280px)",
    gap: "16px"
  },
  card: {
    background: "rgba(31, 25, 20, 0.9)",
    border: "1px solid rgba(255, 196, 122, 0.18)",
    borderRadius: "18px",
    padding: "16px",
    boxShadow: "0 16px 32px rgba(0,0,0,0.22)"
  },
  sectionTitle: {
    marginTop: 0,
    fontSize: "15px",
    textTransform: "uppercase",
    letterSpacing: "0.08em",
    color: "#f1b677"
  },
  functionButton: {
    display: "block",
    width: "100%",
    textAlign: "left",
    marginBottom: "8px",
    padding: "10px 12px",
    color: "#f6f1e8",
    background: "#241d18",
    border: "1px solid rgba(255,255,255,0.08)",
    borderRadius: "12px",
    cursor: "pointer"
  },
  functionButtonActive: {
    background: "#5b2e1b",
    borderColor: "#f1b677"
  },
  list: {
    display: "grid",
    gap: "8px"
  },
  row: {
    display: "grid",
    gap: "4px",
    padding: "10px 12px",
    borderRadius: "12px",
    background: "#221a16"
  },
  pre: {
    whiteSpace: "pre-wrap",
    margin: 0,
    lineHeight: 1.5
  }
};
