import * as vscode from "vscode";

const symbolPatterns: Array<{
  regex: RegExp;
  kind: vscode.SymbolKind;
}> = [
  { regex: /^\s*defmod\s+([A-Za-z_][A-Za-z0-9_]*)/, kind: vscode.SymbolKind.Module },
  { regex: /^\s*defstruct\s+([A-Za-z_][A-Za-z0-9_]*)/, kind: vscode.SymbolKind.Struct },
  { regex: /^\s*defrecord\s+([A-Za-z_][A-Za-z0-9_]*)/, kind: vscode.SymbolKind.Struct },
  { regex: /^\s*deferror\s+([A-Za-z_][A-Za-z0-9_]*)/, kind: vscode.SymbolKind.Event },
  { regex: /^\s*defenum\s+([A-Za-z_][A-Za-z0-9_]*)/, kind: vscode.SymbolKind.Enum },
  { regex: /^\s*impl\s+([A-Za-z_][A-Za-z0-9_:]*)/, kind: vscode.SymbolKind.Object },
  { regex: /^\s*def\s+([A-Za-z_][A-Za-z0-9_]*)/, kind: vscode.SymbolKind.Function }
];

export function activate(context: vscode.ExtensionContext): void {
  const provider = vscode.languages.registerDocumentSymbolProvider(
    { language: "surtr" },
    {
      provideDocumentSymbols(document): vscode.DocumentSymbol[] {
        const symbols: vscode.DocumentSymbol[] = [];
        for (let lineNumber = 0; lineNumber < document.lineCount; lineNumber += 1) {
          const line = document.lineAt(lineNumber);
          for (const pattern of symbolPatterns) {
            const match = line.text.match(pattern.regex);
            if (!match) {
              continue;
            }
            const name = match[1];
            const range = line.range;
            symbols.push(new vscode.DocumentSymbol(name, "", pattern.kind, range, range));
            break;
          }
        }
        return symbols;
      }
    }
  );

  context.subscriptions.push(provider);
}

export function deactivate(): void {}
