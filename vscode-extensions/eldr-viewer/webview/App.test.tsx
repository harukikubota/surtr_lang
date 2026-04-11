import React from "react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { App } from "./App";
import type { ViewerFile } from "../src/viewerTypes";

const viewer: ViewerFile = {
  schema_version: 1,
  format: "eldr_viewer",
  header: {
    magic: "ELDR",
    version: 1,
    debug_level: 2,
    num_chunks: 4
  },
  chunks: [],
  functions: [
    {
      function_id: "fn:0",
      fun_idx: 0,
      name: "Main::entry",
      entry_pc: 0,
      end_pc: 1,
      arity: 0,
      num_locals: 1,
      chunk_id: "Func",
      source_ref: { source_id: "0", span_start: 0, span_end: 5, line: 1, column: 1 },
      opcode_pcs: [0, 1]
    }
  ],
  constants: [{ kind: "Str", idx: 0, value: "hello" }],
  opcodes: [
    { pc: 0, function_id: "fn:0", op: { kind: "LoadConst", const_idx: 0 }, source_ref: null, label: null },
    { pc: 1, function_id: "fn:0", op: { kind: "Halt" }, source_ref: null, label: null }
  ],
  sources: [
    {
      source_id: "0",
      name: "sample.srt",
      normalized_path: "sample.srt",
      content_hash: null,
      text: "print(\"hello\")"
    }
  ],
  errors: []
};

describe("App", () => {
  it("renders the key viewer panes", () => {
    render(<App viewer={viewer} />);
    expect(screen.getByText("Eldr Viewer")).toBeDefined();
    expect(screen.getByText("Functions")).toBeDefined();
    expect(screen.getByText("Opcodes")).toBeDefined();
    expect(screen.getByText("Constants")).toBeDefined();
    expect(screen.getByText("Main::entry")).toBeDefined();
    expect(screen.getByText("LoadConst")).toBeDefined();
  });
});
