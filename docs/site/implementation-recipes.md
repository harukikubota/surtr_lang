# Surtr Implementation Recipes

このドキュメントは、Surtr に新しい命令や builtin を追加するときの実務手順をまとめたものです。

設計の考え方そのものは [コンパイラ設計ガイド](./compiler-design.md) を参照してください。ここでは、実際にどのファイルをどう触るかを短く整理します。

## 1. まず何を追加するか決める

新機能は、まず `Opcode` にするか `builtin` にするかを決めます。

### `Opcode` に向くもの

- 単相
- 頻出
- 副作用なし
- VM が直接処理したほうが自然

例:

- `Int` / `Float` の基本算術
- `String` の `++`
- 基本比較
- 将来のビット演算

### `builtin` に向くもの

- 副作用あり
- 多相
- 内部補助として閉じ込めたい
- Surtr source 上に見せる公開 API として持ちたい

例:

- `print`
- `to_string`
- `inspect`
- `eprint`
- `safe_div`

`safe_div` のように Surtr で書けそうでも、処理系都合の内部操作としてユーザへ露出したくないものは builtin 側へ寄せます。

## 2. 命令追加の手順

命令追加は、共通 IR から VM 実装まで順に下ろしていきます。

### 2.1 `Opcode` を追加する

変更先:

- [`crates/sindr/src/ir.rs`](/Users/haruca/work/rust/surtr/crates/sindr/src/ir.rs)

やること:

- `Opcode` enum に新命令を追加する
- 必要なオペランドを決める
- 既存命令との責務境界を確認する

### 2.2 型規則を追加する

変更先:

- [`crates/scar/src/checker.rs`](/Users/haruca/work/rust/surtr/crates/scar/src/checker.rs)

やること:

- その演算がどの型で許可されるかを決める
- 戻り値型を決める
- 型エラー時の診断を追加する

### 2.3 codegen を追加する

変更先:

- [`crates/forge/src/codegen.rs`](/Users/haruca/work/rust/surtr/crates/forge/src/codegen.rs)

やること:

- 対応する `TypedNode` から新しい opcode を emit する
- 二項演算なら既存の opcode 選択分岐へ追加する
- span が必要なら既存命令と同じ流儀で保持する

### 2.4 VM 実行を追加する

変更先:

- [`crates/eldr/src/vm.rs`](/Users/haruca/work/rust/surtr/crates/eldr/src/vm.rs)

やること:

- `match Opcode::...` に分岐を追加する
- スタック入出力を明確にする
- 不正値や不正状態を `RuntimeError` にする

### 2.5 必要なら値表現を調整する

変更先:

- [`crates/eldr/src/value.rs`](/Users/haruca/work/rust/surtr/crates/eldr/src/value.rs)

やること:

- 新しいランタイム値が必要な場合だけ追加する
- 表示や比較への影響も確認する

### 2.6 ドキュメントとテストを更新する

最低限の変更先:

- [`doc/EldrVM_spec.md`](/Users/haruca/work/rust/surtr/doc/EldrVM_spec.md)
- 該当 crate の unit test
- `rune` の spec / compile_errors

確認したいこと:

- opcode が期待どおり emit される
- VM 実行結果が正しい
- エラー系が想定どおり落ちる

## 3. builtin 追加の手順

builtin は共有メタデータを正本にして、宣言層と VM 実装を追従させます。

### 3.1 canonical metadata を追加する

変更先:

- [`crates/sindr/src/builtin.rs`](/Users/haruca/work/rust/surtr/crates/sindr/src/builtin.rs)

やること:

- `name`
- `builtin_id`
- `arity`
- `sig_str`

注意:

- `builtin_id` は定義順と一致させる
- ここが正本であり、他 crate はこの定義を参照する

### 3.2 標準モジュール側の宣言を追加する

変更先:

- cross-cutting builtin なら [`lib/kernel.srt`](/Users/haruca/work/rust/surtr/lib/kernel.srt)
- type-owned builtin なら対応する type module file
  - 例: [`lib/int.srt`](/Users/haruca/work/rust/surtr/lib/int.srt)
  - 例: [`lib/result.srt`](/Users/haruca/work/rust/surtr/lib/result.srt)
  - 例: [`lib/list.srt`](/Users/haruca/work/rust/surtr/lib/list.srt)

やること:

- `print` のような cross-cutting builtin なら `kernel.srt` の `defmod Kernel` に `@@builtin def ...` を追加する
- builtin type を増減するなら対応 file のトップレベル `@@builtin type ...` を更新する
- `Unit` の builtin type は `kernel.srt` で扱う
- module API を追加するなら `defmod Name { ... }` 側も合わせて更新する

注意:

- これは宣言層であって builtin の正本ではない
- ユーザ source に同様の宣言を足すのではない
- `Result<$T>` と `List<$A>` のような canonical head は compiler 側契約と一致している必要がある
- `Ok` / `Err` は `result.srt` の `@@builtin type ...` special contract として宣言する

### 3.3 Eldr の実装を追加する

変更先:

- [`crates/eldr/src/builtin.rs`](/Users/haruca/work/rust/surtr/crates/eldr/src/builtin.rs)

やること:

- `BUILTIN_IMPLS` に追加する
- 本体関数を実装する
- arity と実装数の整合を保つ

### 3.4 必要なら型規則を追加する

通常は共有 metadata から型検査できるため、大きな変更は不要です。

変更が必要になる例:

- 特殊な多相制約がある
- 引数の組み合わせに追加ルールがある
- builtin 固有の診断を出したい

変更先:

- [`crates/scar/src/checker.rs`](/Users/haruca/work/rust/surtr/crates/scar/src/checker.rs)

### 3.5 codegen 経路を確認する

変更先:

- [`crates/forge/src/codegen.rs`](/Users/haruca/work/rust/surtr/crates/forge/src/codegen.rs)

普通は追加変更不要です。

理由:

- Forge は builtin 名から `builtin_id` を引いて `CallBuiltin` を出すため
- metadata 追加だけでつながることが多いため

### 3.6 ドキュメントとテストを更新する

最低限の変更先:

- [`doc/要件定義v9.md`](/Users/haruca/work/rust/surtr/doc/要件定義v9.md)
- [`doc/EldrVM_spec.md`](/Users/haruca/work/rust/surtr/doc/EldrVM_spec.md)
- [`lib/kernel.srt`](/Users/haruca/work/rust/surtr/lib/kernel.srt) または対応する `lib/*.srt`
- [`docs/site/standard-library.md`](/Users/haruca/work/rust/surtr/docs/site/standard-library.md)
- Eldr builtin 単体テスト
- spec / compile_errors

追加で確認したいこと:

- `@@doc` が source 変更に追従している
- `.eldr` の docs metadata に公開したい説明だけが載る

## 4. 実装順序のおすすめ

### 命令追加

```text
sindr::ir
-> scar::checker
-> forge::codegen
-> eldr::vm
-> docs
-> tests
```

### builtin 追加

```text
sindr::builtin
-> lib/kernel.srt / 対応する lib/<type>.srt
-> eldr::builtin
-> 必要なら scar::checker
-> docs
-> tests
```

## 5. テンプレート

以下は、そのまま作業開始時の下書きとして使えるテンプレートです。

### 5.1 新しい `Opcode` を追加するとき

```md
## Opcode 追加メモ

- 追加する命令名:
- 目的:
- 単相か:
- 副作用はあるか:
- 既存 builtin ではなく opcode にする理由:

### 変更ファイル

- `crates/sindr/src/ir.rs`
- `crates/scar/src/checker.rs`
- `crates/forge/src/codegen.rs`
- `crates/eldr/src/vm.rs`
- 必要なら `crates/eldr/src/value.rs`
- `doc/EldrVM_spec.md`

### 実装チェック

- opcode を追加した
- 型規則を追加した
- codegen を追加した
- VM 実装を追加した
- unit test を追加した
- spec / compile_errors を追加した
```

### 5.2 新しい builtin を追加するとき

```md
## builtin 追加メモ

- builtin 名:
- 目的:
- arity:
- 型シグネチャ:
- ユーザ公開 API か、内部補助か:
- opcode ではなく builtin にする理由:

### 変更ファイル

- `crates/sindr/src/builtin.rs`
- `lib/bootstrap.srt`
- `crates/eldr/src/builtin.rs`
- 必要なら `crates/scar/src/checker.rs`
- `doc/要件定義v9.md`
- `doc/EldrVM_spec.md`

### 実装チェック

- metadata を追加した
- `@@builtin def` 宣言を追加した
- Eldr 実装を追加した
- metadata と実装数の整合を確認した
- unit test を追加した
- spec / compile_errors を追加した
```

### 5.3 追加方式の判断テンプレート

```md
## 方式判断メモ

- 機能名:
- ユーザに名前付き API として見せたいか:
- 単相か多相か:
- 頻出か:
- 副作用はあるか:
- VM が直接扱うと単純になるか:

### 判定

- `Opcode` / `builtin` / `Kernel` 純粋関数

### 理由

- 
```

## 6. 最後に見る観点

- その機能は本当にユーザ公開 API か
- 処理系内部都合を user surface に漏らしていないか
- `Bootstrap` と `Kernel` の責務を壊していないか
- 既存の Phase 範囲外機能を混入させていないか
- Rust 実装依存で進める箇所と、言語仕様として保証する箇所を混同していないか
- 最適化検討を正本仕様へ混ぜていないか

最適化方針が未確定なら、正本 (`doc/要件定義v9.md`) へは入れず、
[`doc/open-issues.md`](/Users/haruca/work/rust/surtr/doc/open-issues.md) に open issue として追跡します。
