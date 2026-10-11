const assert = require("node:assert/strict");
const Module = require("node:module");
const { test } = require("node:test");

test("terminal commands pass compiler and file paths as literal process arguments", async () => {
  const compilerPath = "/opt/surtr tools/$(echo compiler)";
  const filePath = "/work/source $(echo file).srt";
  const commands = new Map();
  const terminals = [];
  const vscode = {
    StatusBarAlignment: { Left: 1 },
    languages: { createDiagnosticCollection: () => ({}) },
    workspace: {
      workspaceFolders: [{ uri: { fsPath: "/work" } }],
      getConfiguration: () => ({ get: (key) => key === "surtr.compiler.path" ? compilerPath : true }),
      onDidSaveTextDocument: () => ({}),
      onDidOpenTextDocument: () => ({}),
      onDidCloseTextDocument: () => ({}),
      onDidChangeConfiguration: () => ({})
    },
    window: {
      activeTextEditor: {
        document: { languageId: "surtr", uri: { scheme: "file", fsPath: filePath } }
      },
      createStatusBarItem: () => ({ show() {} }),
      onDidChangeActiveTextEditor: () => ({}),
      createTerminal: (options) => {
        terminals.push(options);
        return {
          show() {},
          sendText() { throw new Error("terminal input would be parsed by a shell"); }
        };
      }
    },
    commands: {
      registerCommand: (name, handler) => {
        commands.set(name, handler);
        return {};
      }
    }
  };
  const originalLoad = Module._load;
  Module._load = function (request, parent, isMain) {
    return request === "vscode" ? vscode : originalLoad.call(this, request, parent, isMain);
  };
  let extension;
  try {
    extension = require("../dist/extension.js");
  } finally {
    Module._load = originalLoad;
  }
  extension.activate({ subscriptions: [] });

  await commands.get("surtr.run.file")();
  await commands.get("surtr.build.file")();
  await commands.get("surtr.test.workspace")();

  assert.deepEqual(terminals, [
    { name: "Surtr", shellPath: compilerPath, shellArgs: ["run", filePath], cwd: "/work" },
    { name: "Surtr", shellPath: compilerPath, shellArgs: ["build", filePath], cwd: "/work" },
    { name: "Surtr", shellPath: compilerPath, shellArgs: ["test"], cwd: "/work" }
  ]);
});
