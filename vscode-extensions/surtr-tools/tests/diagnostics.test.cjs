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
    show(document) {
      vscode.window.activeTextEditor = document ? { document } : undefined;
      events.editor(vscode.window.activeTextEditor);
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
  h.show(document);
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
  h.show(document);
  document.version += 1;
  h.complete(0, "obsolete diagnostic");
  await flush();
  assert.equal(h.messages(document), undefined);
});

test("disabling diagnostics clears results immediately and invalidates in-flight checks", async () => {
  const h = harness();
  const document = h.document();
  h.show(document);
  h.complete(0, "published diagnostic");
  await flush();
  h.events.save(document);
  h.configure("surtr.diagnostics.onSave", false);
  assert.equal(h.status.text, "$(flame) Surtr");
  assert.equal(h.messages(document), undefined);
  h.complete(1, "late diagnostic");
  await flush();
  assert.equal(h.messages(document), undefined);
});

test("reenabling diagnostics never reuses a previous request identity", async () => {
  const h = harness();
  const document = h.document();
  h.show(document);
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
  h.show(document);
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
  h.show(document);
  h.complete(0, "published diagnostic");
  await flush();
  h.events.save(document);
  document.isClosed = true;
  h.events.close?.(document);
  assert.equal(h.status.text, "$(flame) Surtr");
  assert.equal(h.messages(document), undefined);
  const reopened = h.document();
  h.show(reopened);
  h.complete(2, "reopened diagnostic");
  await flush();
  h.complete(1, "closed diagnostic");
  await flush();
  assert.deepEqual(h.messages(reopened), ["reopened diagnostic"]);
});

test("obsolete failures do not warn or overwrite the current status", async () => {
  const h = harness();
  const document = h.document();
  h.show(document);
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


test("background diagnostics keep the active document status and are restored on switching back", async () => {
  const h = harness();
  const a = h.document("/work/a.srt");
  const b = h.document("/work/b.srt");
  h.show(a);
  h.show(b);
  h.complete(1);
  await flush();
  assert.equal(h.status.text, "$(pass) Surtr");
  h.complete(0, "A diagnostic");
  await flush();
  assert.deepEqual(h.messages(a), ["A diagnostic"]);
  assert.deepEqual(h.messages(b), []);
  assert.equal(h.status.text, "$(pass) Surtr");
  h.show(a);
  assert.equal(h.status.text, "$(error) Surtr 1");
  h.show(b);
  assert.equal(h.status.text, "$(pass) Surtr");
});

test("background failures do not replace the active document status", async () => {
  const h = harness();
  const a = h.document("/work/a.srt");
  const b = h.document("/work/b.srt");
  h.show(a);
  h.show(b);
  h.complete(1);
  await flush();
  h.pending[0].reject(new Error("A compiler failed"));
  await flush();
  assert.equal(h.status.text, "$(pass) Surtr");
  h.show(a);
  assert.equal(h.status.text, "$(warning) Surtr");
});

test("no editor and unsupported editors retain neutral status during background checks", async () => {
  const h = harness();
  const a = h.document("/work/a.srt");
  h.show(a);
  h.complete(0, "A diagnostic");
  await flush();
  assert.equal(h.status.text, "$(error) Surtr 1");
  h.show();
  assert.equal(h.status.text, "$(flame) Surtr");
  h.events.save(a);
  h.complete(1, "A updated diagnostic");
  await flush();
  assert.equal(h.status.text, "$(flame) Surtr");
  h.show(a);
  assert.equal(h.status.text, "$(error) Surtr 1");
  const text = h.document("/work/notes.txt");
  text.languageId = "plaintext";
  h.show(text);
  assert.equal(h.status.text, "$(flame) Surtr");
  h.complete(2);
  await flush();
  assert.equal(h.status.text, "$(flame) Surtr");
  h.show(a);
  assert.equal(h.status.text, "$(pass) Surtr");
  const untitled = h.document("/unsaved.srt");
  untitled.uri.scheme = "untitled";
  h.show(untitled);
  assert.equal(h.status.text, "$(flame) Surtr");
});

test("cached status is invalidated by compiler configuration and document edits", async () => {
  const h = harness();
  const document = h.document();
  h.show(document);
  h.complete(0, "old diagnostic");
  await flush();
  h.configure("surtr.compiler.path", "/new/surtr");
  assert.equal(h.status.text, "$(flame) Surtr");
  h.show(document);
  assert.equal(h.status.text, "$(flame) Surtr");
  h.complete(2);
  await flush();
  assert.equal(h.status.text, "$(pass) Surtr");
  document.version += 1;
  h.show(document);
  assert.equal(h.status.text, "$(flame) Surtr");
});
