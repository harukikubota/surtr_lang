import { execFile } from "node:child_process";
import { promisify } from "node:util";
import * as vscode from "vscode";

const execFileAsync = promisify(execFile);

type CheckError = {
  kind: string;
  phase: string;
  line: number;
  column: number;
  span: [number, number];
  message: string;
  expected?: string;
  got?: string;
  hint?: string;
};

type CheckReport = {
  errors: CheckError[];
};

export function activate(context: vscode.ExtensionContext): void {
  const diagnostics = vscode.languages.createDiagnosticCollection("surtr");
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 10);
  status.name = "Surtr Diagnostics";
  status.text = "$(flame) Surtr";
  status.show();
  const requests = new Map<string, symbol>();
  const documentStatuses = new Map<string, { version: number; text: string }>();
  const updateStatus = (): void => {
    const document = vscode.window.activeTextEditor?.document;
    if (!document || document.languageId !== "surtr" || document.uri.scheme !== "file" ||
        document.isClosed ||
        !vscode.workspace.getConfiguration().get<boolean>("surtr.diagnostics.onSave", true)) {
      status.text = "$(flame) Surtr";
      return;
    }
    const saved = documentStatuses.get(document.uri.toString());
    status.text = saved?.version === document.version ? saved.text : "$(flame) Surtr";
  };

  const refreshDiagnostics = async (document: vscode.TextDocument): Promise<void> => {
    if (document.languageId !== "surtr" || document.uri.scheme !== "file" || document.isClosed) {
      return;
    }

    const key = document.uri.toString();
    const diagnosticsEnabled = vscode.workspace
      .getConfiguration()
      .get<boolean>("surtr.diagnostics.onSave", true);
    if (!diagnosticsEnabled) {
      requests.delete(key);
      diagnostics.delete(document.uri);
      documentStatuses.delete(key);
      updateStatus();
      return;
    }

    const request = Symbol();
    const version = document.version;
    const compilerPath = configuredCompilerPath();
    requests.set(key, request);
    const isCurrent = (): boolean =>
      requests.get(key) === request &&
      !document.isClosed &&
      document.version === version &&
      vscode.workspace.getConfiguration().get<boolean>("surtr.diagnostics.onSave", true) &&
      configuredCompilerPath() === compilerPath;

    try {
      const report = await runCheck(document.uri.fsPath, compilerPath);
      if (!isCurrent()) {
        return;
      }
      const nextDiagnostics = report.errors.map((error) => {
        const line = Math.max(0, error.line - 1);
        const column = Math.max(0, error.column - 1);
        const range = new vscode.Range(line, column, line, column + 1);
        const diagnostic = new vscode.Diagnostic(range, error.message, vscode.DiagnosticSeverity.Error);
        diagnostic.source = `surtr:${error.phase}`;
        diagnostic.code = error.kind;
        if (error.hint) {
          diagnostic.relatedInformation = [
            new vscode.DiagnosticRelatedInformation(
              new vscode.Location(document.uri, range),
              error.hint
            )
          ];
        }
        return diagnostic;
      });
      diagnostics.set(document.uri, nextDiagnostics);
      documentStatuses.set(key, {
        version,
        text: nextDiagnostics.length === 0
          ? "$(pass) Surtr"
          : `$(error) Surtr ${nextDiagnostics.length}`
      });
      updateStatus();
    } catch (error) {
      if (!isCurrent()) {
        return;
      }
      documentStatuses.set(key, { version, text: "$(warning) Surtr" });
      updateStatus();
      void vscode.window.showWarningMessage(String(error));
    }
  };

  context.subscriptions.push(
    diagnostics,
    status,
    { dispose: () => { requests.clear(); documentStatuses.clear(); } },
    vscode.workspace.onDidSaveTextDocument((document) => {
      void refreshDiagnostics(document);
    }),
    vscode.workspace.onDidOpenTextDocument((document) => {
      void refreshDiagnostics(document);
    }),
    vscode.workspace.onDidCloseTextDocument((document) => {
      requests.delete(document.uri.toString());
      diagnostics.delete(document.uri);
      documentStatuses.delete(document.uri.toString());
      updateStatus();
    }),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (!event.affectsConfiguration("surtr.diagnostics.onSave") &&
          !event.affectsConfiguration("surtr.compiler.path")) {
        return;
      }
      requests.clear();
      diagnostics.clear();
      documentStatuses.clear();
      updateStatus();
      for (const document of vscode.workspace.textDocuments) {
        void refreshDiagnostics(document);
      }
    }),
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      updateStatus();
      if (editor?.document) {
        void refreshDiagnostics(editor.document);
      }
    }),
    vscode.commands.registerCommand("surtr.run.file", async () => {
      await runCliWithTerminal(["run", currentFilePath("surtr")]);
    }),
    vscode.commands.registerCommand("surtr.build.file", async () => {
      await runCliWithTerminal(["build", currentFilePath("surtr")]);
    }),
    vscode.commands.registerCommand("surtr.test.workspace", async () => {
      await runCliWithTerminal(["test"]);
    }),
    vscode.commands.registerCommand("surtr.bytecode.dumpJson", async () => {
      const target = currentFilePath();
      const { stdout } = await execSurtr(["dump", target, "--format", "json"]);
      const document = await vscode.workspace.openTextDocument({
        language: "json",
        content: stdout
      });
      await vscode.window.showTextDocument(document, { preview: false });
    })
  );
}

export function deactivate(): void {}

async function runCheck(filePath: string, compilerPath: string): Promise<CheckReport> {
  try {
    const { stdout } = await execSurtr(["check", filePath, "--format", "json"], compilerPath);
    return JSON.parse(stdout) as CheckReport;
  } catch (error) {
    const stdout = stdoutFromError(error);
    if (!stdout) {
      throw error;
    }
    return JSON.parse(stdout) as CheckReport;
  }
}

function configuredCompilerPath(): string {
  return vscode.workspace
    .getConfiguration()
    .get<string>("surtr.compiler.path", "surtr");
}

async function execSurtr(
  args: string[],
  compilerPath = configuredCompilerPath()
): Promise<{ stdout: string; stderr: string }> {
  return execFileAsync(compilerPath, args, {
    cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath
  });
}

async function runCliWithTerminal(args: string[]): Promise<void> {
  const compilerPath = vscode.workspace
    .getConfiguration()
    .get<string>("surtr.compiler.path", "surtr");
  const terminal = vscode.window.createTerminal({
    name: "Surtr",
    shellPath: compilerPath,
    shellArgs: args,
    cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath
  });
  terminal.show();
}

function currentFilePath(expectedLanguageId?: string): string {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.uri.scheme !== "file") {
    throw new Error("No active file is available.");
  }
  if (expectedLanguageId && editor.document.languageId !== expectedLanguageId) {
    throw new Error("The active editor is not a Surtr file.");
  }
  return editor.document.uri.fsPath;
}

function stdoutFromError(error: unknown): string | undefined {
  if (!error || typeof error !== "object") {
    return undefined;
  }
  const stdout = "stdout" in error ? (error.stdout as string | Buffer | undefined) : undefined;
  if (typeof stdout === "string") {
    return stdout;
  }
  if (Buffer.isBuffer(stdout)) {
    return stdout.toString("utf8");
  }
  return undefined;
}
