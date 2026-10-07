const assert = require("node:assert/strict");
const Module = require("node:module");
const { test } = require("node:test");
const { promisify } = require("node:util");

function harness() {
  const events = {};
  const pending = [];
  const published = new Map();
  const warnings = [];
  const settings = { "surtr.compiler.path": "surtr", "surtr.diagnostics.onSave": true };
  const documents = [];
  const status = { show() {} };
  const register = (name) => (listener) => {
    events[name] = listener;
    return { dispose() {} };
  };
  const vscode = {
    StatusBarAlignment: { Left: 1 },
    DiagnosticSeverity: { Error: 0 },
    Range: class { constructor(...positions) { this.positions = positions; } },
    Diagnostic: class {
      constructor(range, message, severity) { Object.assign(this, { range, message, severity }); }
    },
    languages: {
      createDiagnosticCollection: () => ({
        set: (uri, value) => published.set(uri.toString(), value),
        delete: (uri) => published.delete(uri.toString()),
        clear: () => published.clear(),
        dispose() {}
      })
    },
    workspace: {
      textDocuments: documents,
      workspaceFolders: [{ uri: { fsPath: "/work" } }],
      getConfiguration: () => ({ get: (key, fallback) => settings[key] ?? fallback }),
      onDidSaveTextDocument: register("save"),
      onDidOpenTextDocument: register("open"),
      onDidCloseTextDocument: register("close"),
      onDidChangeConfiguration: register("configuration")
    },
    window: {
      createStatusBarItem: () => status,
      onDidChangeActiveTextEditor: register("editor"),
      showWarningMessage: (message) => warnings.push(message)
    },
    commands: { registerCommand: () => ({ dispose() {} }) }
  };
  const execFile = () => { throw new Error("use promisified execFile"); };
  execFile[promisify.custom] = (file, args, options) => new Promise((resolve, reject) => {
    pending.push({ file, args, options, resolve, reject });
  });
  const extensionPath = require.resolve("../dist/extension.js");
  delete require.cache[extensionPath];
  const originalLoad = Module._load;
  Module._load = function (request, parent, isMain) {
    if (request === "vscode") return vscode;
    if (request === "node:child_process") return { execFile };
    return originalLoad.call(this, request, parent, isMain);
  };
  let extension;
  try {
    extension = require(extensionPath);
  } finally {
    Module._load = originalLoad;
  }
  extension.activate({ subscriptions: [] });
  return {
    events, pending, published, warnings, status,
    document(path = "/work/test.srt") {
      const document = {
        uri: { scheme: "file", fsPath: path, toString: () => `file://${path}` },
        languageId: "surtr", version: 1, isClosed: false
      };
      documents.push(document);
      return document;
    },
    configure(key, value) {
      settings[key] = value;
      events.configuration?.({ affectsConfiguration: (section) => section === key });
    },
    complete(index, message) {
      const errors = message ? [{ kind: "SyntaxError", phase: "parse", line: 1, column: 1, span: [0, 1], message }] : [];
      pending[index].resolve({ stdout: JSON.stringify({ errors }), stderr: "" });
    },
    messages(document) { return published.get(document.uri.toString())?.map((d) => d.message); }
  };
}

const flush = () => new Promise((resolve) => setImmediate(resolve));

test("newer diagnostics survive out-of-order checks for one document", async () => {
  const h = harness();
  const document = h.document();
  h.events.open(document);
  document.version += 1;
  h.events.save(document);
  h.complete(1, "new diagnostic");
  await flush();
  h.complete(0, "old diagnostic");
  await flush();
  assert.deepEqual(h.messages(document), ["new diagnostic"]);
});

test("an edited document rejects a pending result before another save", async () => {
  const h = harness();
  const document = h.document();
  h.events.open(document);
  document.version += 1;
  h.complete(0, "obsolete diagnostic");
  await flush();
  assert.equal(h.messages(document), undefined);
});

test("disabling diagnostics clears results immediately and invalidates in-flight checks", async () => {
  const h = harness();
  const document = h.document();
  h.events.open(document);
  h.complete(0, "published diagnostic");
  await flush();
  h.events.save(document);
  h.configure("surtr.diagnostics.onSave", false);
  assert.equal(h.messages(document), undefined);
  h.complete(1, "late diagnostic");
  await flush();
  assert.equal(h.messages(document), undefined);
});

test("reenabling diagnostics never reuses a previous request identity", async () => {
  const h = harness();
  const document = h.document();
  h.events.open(document);
  h.configure("surtr.diagnostics.onSave", false);
  h.configure("surtr.diagnostics.onSave", true);
  assert.equal(h.pending.length, 2);
  h.complete(1, "enabled diagnostic");
  await flush();
  h.complete(0, "disabled request diagnostic");
  await flush();
  assert.deepEqual(h.messages(document), ["enabled diagnostic"]);
});

test("compiler configuration changes discard results from the previous compiler", async () => {
  const h = harness();
  const document = h.document();
  h.events.open(document);
  h.configure("surtr.compiler.path", "/new/surtr");
  assert.equal(h.pending.length, 2);
  assert.equal(h.pending[1].file, "/new/surtr");
  h.complete(1, "new compiler diagnostic");
  await flush();
  h.complete(0, "old compiler diagnostic");
  await flush();
  assert.deepEqual(h.messages(document), ["new compiler diagnostic"]);
});

test("closing and reopening the same URI rejects the closed document's result", async () => {
  const h = harness();
  const document = h.document();
  h.events.open(document);
  h.complete(0, "published diagnostic");
  await flush();
  h.events.save(document);
  document.isClosed = true;
  h.events.close?.(document);
  assert.equal(h.messages(document), undefined);
  const reopened = h.document();
  h.events.open(reopened);
  h.complete(2, "reopened diagnostic");
  await flush();
  h.complete(1, "closed diagnostic");
  await flush();
  assert.deepEqual(h.messages(reopened), ["reopened diagnostic"]);
});

test("obsolete failures do not warn or overwrite the current status", async () => {
  const h = harness();
  const document = h.document();
  h.events.open(document);
  h.events.save(document);
  h.complete(1);
  await flush();
  const currentStatus = h.status.text;
  h.pending[0].reject(new Error("obsolete failure"));
  await flush();
  assert.deepEqual(h.warnings, []);
  assert.equal(h.status.text, currentStatus);
  h.events.save(document);
  h.pending[2].reject(new Error("current failure"));
  await flush();
  assert.deepEqual(h.warnings, ["Error: current failure"]);
});
