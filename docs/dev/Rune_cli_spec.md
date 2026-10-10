# Rune CLI Spec

`Rune` の CLI surface と dispatch 契約をまとめる開発者向け仕様。

- CLI の意味契約は本書、現行 command synopsis は [`crates/rune/README.md`](../../crates/rune/README.md) と引数 parser を正本とする
- 本書は `Rune` の command dispatch、script 実行入口、テスト固定点を定める

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

## 6. Test コマンド

`surtr test <file-path> [--test TEXT] [--describe TEXT] [--it TEXT] [--include-xit] [--deny-pending] [--list] [--timings] [--quiet|-q] [--format human|json]`。
単体は実ファイルパスを受け取り、相対パスは起動時CWDを基準にする。絶対・親ディレクトリ・symlinkも通常の指定として読み込む。前後空白や区切り文字を変換せず、拡張子補完や旧selectorへのfallbackをしない。
includeは入口の位置を基準に解決し、依存内容をfingerprintへ反映する。

開発用debugビルドの標準テストでは `surtr test --all` を使う。`lib/tests/*/*.srt` の入口を辞書順に一度ずつ列挙し、カテゴリ名・basenameで除外せず、root直下と3成分以上のsupportを実行しない。対象0件は配置・対象選択エラーで終了コード1。切れたsymlinkとdirectory symlinkは共通の読み込み経路で失敗として報告する。
単体と標準allは同じ読み込み経路と依存fingerprintを使い、各入口を独立してコンパイル・実行する。標準allはファイル異常後も次の入口へ進み、実行異常の起きたVMは再利用しない。配置と観測の方針は [テスト方針](./テスト方針.md#38-標準ライブラリ-test-script) を参照する。
リリースビルドにおけるallの振る舞いは未確定とする。

標準ソースの登録と標準環境の構築はプロセス内で共有する。各入口は標準環境の型検査checkpointを復元し、固有のincludeと本体をコンパイルする。同じ依存内容・stage順序・SourceId配置のincludeはコンパイル済みprefixを共有するが、入口の型検査状態とVMは共有しない。bytecodeキャッシュのライブラリ指紋はコマンド内で一度計算する。

stderrが端末で、端末幅を取得できる場合は、Cargo形式の `Preparing` / `Compiling` / `Running` を一行の一時進捗として表示する。`Preparing` は最初のbytecodeキャッシュミス時の標準環境準備、残る二つはファイル番号・総数・パスを表示する。`--quiet` と `--format json` でも表示し、最終結果・診断の描画前に消去する。長い表示は端末幅内に省略する。stderrが非端末の場合は一時行や制御文字を出さず、JSONのstdoutは単一文書を維持する。

値付き引数は空白・等号の両形式を受理する。各オプションは一度だけ指定でき、`--` 以降は位置引数とする。
フィルターは大文字小文字を区別する部分一致。種類間は AND、同種の祖先間はいずれか一致とし、名前の結合文字列では比較しない。
`--list` とフィルターはケース本文だけを抑止する。トップレベルと test/describe の走査は実行する。
xit は理由付き停止、pend は理由付き未実装項目。理由の空白・空文字列とケース本文中のケース宣言は実行異常とする。
明示フィルターの全体一致ゼロ、選択された pend に対する `--deny-pending` はポリシー違反とする。
ケース・スコープ・ファイル・ポリシーの失敗を分離して集計する。JSON は単一文書とし、走査出力を scripts、ケース出力を cases の io に保持する。

### 件数と終了コード

`discovered = selected + filtered`。実行時は `selected = passed + failed + skipped + pending`、
一覧時は `selected = runnable + skipped + pending`。`executed = passed + failed`。
human の `total` は discovered と同じ件数を表示する。
スコープ本文の Err は scope_failures、読み取り・コンパイル・VM 実行異常は script_errors とする。
VM 異常で実行中ケースが中断した場合だけ、そのケースも Failed に確定する。
未到達の宣言は discovered に含めない。

policy_errors は拒否された選択 Pending 一件につき一件、明示フィルターの全体一致ゼロで一件。
走査自体が失敗した場合、一致ゼロの診断は加えない。選択された xit / pend も一致件数に含む。
失敗・異常・ポリシー違反・usage error があれば終了コード1、それ以外は0。

### JSON 文書

トップレベルは command、mode（run/list）、options、scripts、cases、errors、summary、exit_code、duration_ns。
options は target（all/file）、filters（test/describe/it）、include_xit、deny_pending、quiet、timings、format。
引数検証未完了の usage error では mode / options は null。

cases は file、case_index（ファイル内の0始まり宣言順）、scopes（kind/name）、name、declaration（it/xit/pend）、
selected、status、reason、detail、duration_ns、io（stdout/stderr）、diagnostic を持つ。
status は実行時 passed/failed/skipped/pending/filtered、一覧時 runnable/skipped/pending/filtered。
存在しない理由・詳細・診断と未計測時間は null。非実行ケースの io も null とする。
既存の診断情報を渡し、新しい失敗位置は探索しない。

scripts は処理した file、status（completed/aborted）、走査処理の io（stdout/stderr）。
errors は kind（usage/scope/script/policy）、message、利用可能な diagnostic を保持する。
quiet の実行結果では failed と拒否された pending の詳細だけを残す。一覧では選択項目をすべて残す。
summary と errors は省略しない。stdout に走査の生出力・色・human 行を混ぜず、診断を stderr に重複表示しない。

引数解析が失敗しても、一意な有効 `--format json` が共通トークン列にあれば JSON で usage error を出す。
format の重複・不正・欠落で一意に決められなければ stderr に通常の usage error を出す。

### 時間

引数検証成功後から全ファイルの処理と集計終了までをコマンド時間とし、最終出力を含めない。
ケースは本文の評価から Result の返却または実行異常までを測り、IO 初期化と結果の表示は含めない。
Skipped / Pending / Filtered / 一覧のケースは未計測。human は ms、JSON は非負整数の ns。

## 7. Testing

- `Rune` 単体テストでは、実在 file path を与えた dispatch が `run` 経路へ入ることを固定する
- CLI integration では shebang 付き script を直接実行した結果が `surtr run` と一致することを固定する
- shebang 行の parse 無害性は lexer / parser の既存 comment 契約で担保する
- `repl` / `tui` integration では startup failure 時の stderr 形状と exit code を固定する
- `unit/rune` または `unit/xldr` では command error から `RuneError` への変換経路を固定する
