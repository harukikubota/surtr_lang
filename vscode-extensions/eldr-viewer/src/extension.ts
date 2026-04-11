import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";
import * as vscode from "vscode";

import type { ViewerFile } from "./viewerTypes";

const execFileAsync = promisify(execFile);

export function activate(context: vscode.ExtensionContext): void {
  context.subscriptions.push(
    vscode.commands.registerCommand("surtr.bytecode.openViewer", async () => {
      const filePath = currentFilePath();
      const viewerData = await loadViewerData(filePath);
      const panel = vscode.window.createWebviewPanel(
        "surtr-eldr-viewer",
        `Eldr Viewer: ${filePath.split(/[\\/]/).pop()}`,
        vscode.ViewColumn.Beside,
        {
          enableScripts: true
        }
      );

      const scriptPath = join(context.extensionPath, "dist", "webview.js");
      const bundle = await readFile(scriptPath, "utf8");
      panel.webview.html = renderHtml(panel.webview, bundle, viewerData);
    })
  );
}

export function deactivate(): void {}

async function loadViewerData(filePath: string): Promise<ViewerFile> {
  const compilerPath = vscode.workspace
    .getConfiguration()
    .get<string>("surtr.compiler.path", "surtr");
  const { stdout } = await execFileAsync(
    compilerPath,
    ["dump", filePath, "--format", "viewer-json"],
    { cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath }
  );
  return JSON.parse(stdout) as ViewerFile;
}

function renderHtml(webview: vscode.Webview, bundle: string, viewerData: ViewerFile): string {
  const viewerJson = JSON.stringify(viewerData).replace(/</g, "\\u003c");
  return `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Eldr Viewer</title>
  </head>
  <body>
    <div id="root"></div>
    <script>
      window.__SURTR_VIEWER_DATA__ = ${viewerJson};
    </script>
    <script>${bundle}</script>
  </body>
</html>`;
}

function currentFilePath(): string {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.uri.scheme !== "file") {
    throw new Error("No active file is available.");
  }
  return editor.document.uri.fsPath;
}
