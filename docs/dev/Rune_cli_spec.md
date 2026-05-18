# Rune CLI Spec

`Rune` の CLI surface と dispatch 契約をまとめる開発者向け仕様。

- CLI surface の正本は [../../doc/要件定義v9.md](../../doc/要件定義v9.md)
- 本書は `Rune` の command dispatch、script 実行入口、テスト固定点を補足する

---

## 1. 目的と責務

`Rune` は以下を担う。

- `surtr` CLI の引数解析
- `check` / `run` / `build` / `dump` / `test` / `repl` / `tui` の command dispatch
- script file / bytecode file の実行入口
- CLI 境界での usage error と command 単位の option validation

`Rune` は以下を担わない。

- REPL セッション状態
- VM 自体の実行意味論
- parser / resolver / typechecker / codegen の内部契約

---

## 2. Command Dispatch

- 既知の第1引数 `--version`, `check`, `run`, `repl`, `build`, `test`, `dump`, `tui` は、その command として解釈する
- 既知 command でない第1引数が実在する file path の場合、`Rune` は `surtr run <path>` として扱う
- 上記の file path fallback は shebang 経由の直接実行を成立させるための CLI 契約である
- 既知 command でも実在 file path でもない第1引数は usage error とする

この fallback は dispatch 層だけの sugar であり、compile unit kind や source kind の判定を追加で切り替えない。

---

## 3. Script Execution

- `surtr run <file.srt>` は source file を script として読み、既存の script compile pipeline へ渡す
- `surtr <file.srt>` は dispatch 正規化後に `surtr run <file.srt>` と完全に同じ経路へ入る
- script source 先頭の shebang 行は lexer の行コメント規則で無視され、後続の parse / include 収集 / compile 契約を変えない
- shebang は CLI 仕様では `#!/usr/bin/env surtr` を推奨例とするが、OS が `surtr` へ引き渡せる interpreter path であればよい

---

## 4. Shebang Contract

- 利用者は script file の先頭に shebang を置き、実行権限を付けることで `./hello.srt` のように直接起動できる
- OS が shebang を解決すると、`Rune` には `surtr <script-path>` の形で引数が渡される前提で扱う
- `Rune` は shebang 文字列自体を解釈しない。shebang の解釈は OS の責務とする
- `Rune` が保証するのは、OS から渡された script path を `run` command と等価に実行することだけである

---

## 5. Error Handling

- file path fallback 後の option validation は通常の `run` command と同じ規則を使う
- path が存在しない場合、CLI は fallback せず usage error を返す
- `.eldr` file を path fallback で受け取った場合も `run` command と同じ入力種別判定を使う
- `Rune` は `main` で `RuneError::emit()` と exit code を一元処理する唯一の CLI 境界とする
- `xldr` の `cli_command` / `tui::run_command` は typed command error を返し、最終 stderr 出力や process exit を行わない
- `repl` / `tui` の startup failure は `xldr` で typed error を構築し、`Rune` が `RuneError` へ adapter 変換して human diagnostic / plain message / exit code を確定する
- interactive session 開始後の REPL chunk evaluation error は従来どおり session output として扱い、この節の CLI startup failure 契約とは分けて考える

---

## 6. Testing

- `Rune` 単体テストでは、実在 file path を与えた dispatch が `run` 経路へ入ることを固定する
- CLI integration では shebang 付き script を直接実行した結果が `surtr run` と一致することを固定する
- shebang 行の parse 無害性は lexer / parser の既存 comment 契約で担保する
- `repl` / `tui` integration では startup failure 時の stderr 形状と exit code を固定する
- `unit/rune` または `unit/xldr` では command error から `RuneError` への変換経路を固定する
