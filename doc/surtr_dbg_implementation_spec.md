# `dbg!` 実装仕様書

## 目的

`dbg!` は、Surtr VM の実行途中で値を観測するためのデバッグ用特殊構文である。
REPL の通常表示を拡張する機能ではなく、関数内部・クロージャ内部・match 分岐内部・script 実行中など、通常の REPL 評価結果だけでは追跡しにくい位置で値を確認するために使う。

```surtr
dbg!(name, age)
dbg!(calc_score(hand))
dbg!(parsed, yaku, score)
```

## 基本方針

- `dbg!` は公開 builtin ではない。
- `dbg!` は parser が認識する特殊形式とする。
- docs / `:sig` は `lib/bootstrap.srt` に置かれた
  `@intrinsic def dbg!<$A>(values: *$A) -> Unit`
  と `@doc` だけを正本にする。
- この intrinsic signature は通常の callable declaration ではない。
- 実行は専用 `Opcode` で行う。
- 戻り値は常に `Unit` とする。
- `dbg!` の内部テンプレートはユーザから直接呼べない。
- 元の式は ariadne の span / caption / underline で辿れるようにする。
- 表示本文は `型名: inspect` 形式とする。
- `:doc dbg!(...)` / `:sig dbg!(...)` は `Kernel::if` と同様の引数擬似適用経路で
  `Bootstrap::dbg!` に解決する。

## 構文

```surtr
dbg!(expr1, expr2, ...)
```

### 有効例

```surtr
dbg!(name)
dbg!(name, age)
dbg!(calc_score(hand))
dbg!({|x| x})
dbg!(user.name)
```

### REPL での例

```surtr
name = "alice"
dbg!(name)
```

期待される動作:

- `name = "alice"` は通常の binding 表示を行う。
- `dbg!(name)` は debug 出力だけを行う。
- `dbg!(name)` の戻り値は `Unit`。
- top-level expression result が `Unit` の場合、REPL は戻り値を表示しない。

```surtr
x = dbg!(name)
```

期待される動作:

- `dbg!(name)` の debug 出力を行う。
- `x` には `Unit` が束縛される。
- 変数バインドは型による絞込みなく全て表示するため、REPL は `x: Unit = ()` を表示する。

## 型仕様

```text
dbg!(expr...) -> Unit
```

各 `expr` は通常の式として型検査される。
`dbg!` 全体の型は、引数の型に関係なく `Unit` である。

### 関数引数位置

```surtr
foo(dbg!(name))
```

`dbg!(name)` は `Unit` 型の式として扱われる。
したがって、`foo` の該当引数が `Unit` を受け取る場合のみ型検査を通る。

### binding 位置

```surtr
x = dbg!(name)
```

`x` の型は `Unit` になる。
これは仕様通りとする。

## 評価順序

`dbg!` の引数は左から右へ 1 回だけ評価する。

```surtr
dbg!(a(), b(), c())
```

評価順序:

1. `a()` を評価
2. `b()` を評価
3. `c()` を評価
4. 評価値を debug 出力
5. `Unit` を返す

副作用を持つ式が引数にある場合も、通常の式評価順序に従う。

## 表示仕様

### 表示本文

元の式文字列は表示本文に含めない。

```text
String: "alice"
Int: 42
User: User(name: "alice", age: 42)
Function: <function:...>
```

### ariadne 表示

元の式は ariadne の span / caption / underline で示す。

```text
Debug: dbg!
 --> main.srt:12:3
  |
12 |   dbg!(name, calc_score(hand), {|x| x})
  |        ----  ----------------  -------
  |        String: "alice"
  |              Int: 8000
  |                                Function: <function:...>
```

### 出力先

`dbg!` は debug output として扱う。
標準出力に出す `print` とは分ける。

推奨:

- `print(...)`: stdout
- `dbg!(...)`: stderr

理由:

- ユーザ向け出力とデバッグ出力を分離できる。
- テスト時に stdout / stderr を分けて capture できる。
- CLI 実行時に通常出力をパイプしても debug 出力を分離しやすい。

## REPL 表示ポリシーとの関係

REPL 表示ポリシーは次の通りとする。

```text
- top-level expression result が Unit の場合は表示しない
- binding result は型による絞込みをせず、全て表示する
- したがって x = dbg!(...) は x: Unit = () を表示する
```

`dbg!` はこのポリシーに従う通常の `Unit` 式として扱う。
REPL 専用の例外処理は不要。

## 修正範囲

### 1. lexer

対象例:

- `crates/spire/src/token.rs`
- `crates/spire/src/lexer.rs`

追加:

```rust
Token::Bang // !
```

注意:

- `!=` は既存通り `BangEq` として最優先で読む。
- 単独の `!` は `Token::Bang` として読む。
- `dbg!(...)` は `Ident("dbg")`, `Bang`, `LParen`, ... として token 化される。

### 2. AST

対象例:

- `crates/spire/src/ast.rs`

追加案:

```rust
pub struct DbgArg {
    pub span: Span,
    pub expr: Ast,
}

pub enum Ast {
    // ...
    Dbg(Span, Vec<DbgArg>),
}
```

`DbgArg` に式文字列ラベルは持たせない。
必要なのは `span` と `expr` のみ。

### 3. parser

対象例:

- `crates/spire/src/parser/expr.rs`

追加:

```surtr
dbg!(expr, expr, ...)
```

parse 条件:

- `Ident("dbg")` の直後に `Bang` がある。
- さらに `(` または `()` 相当の call 開始が続く。

parse 結果:

```rust
Ast::Dbg(span, args)
```

注意:

- `dbg(...)` は通常の関数呼び出しとして扱う。
- `dbg!` は特殊構文。
- `dbg!()` は不許可とする。少なくとも 1 引数が必要。
- `dbg!(a, b,)` の末尾カンマは既存 call args の規則に合わせる。

### 4. sigil / resolve

対象例:

- `crates/sigil`

対応:

- `Ast::Dbg` の各引数式を通常の式として resolve する。
- `dbg` という名前自体を symbol table に登録しない。
- `dbg!` は名前解決対象ではない。

注意:

- ユーザ定義関数 `dbg` は `dbg(...)` では呼べる。
- `dbg!(...)` は常に特殊構文になる。
- `dbg!` の内部テンプレート名は symbol table に出さない。

### 5. scar / typecheck

対象例:

- `crates/scar/src/typed.rs`
- `crates/scar` の typecheck 実装

追加案:

```rust
pub struct TypedDbgArg {
    pub span: Span,
    pub ty: Ty,
    pub expr: TypedNode,
}

pub enum TypedInner {
    // ...
    Dbg(Vec<TypedDbgArg>),
}
```

型規則:

```text
expr_i : T_i
--------------------------------
dbg!(expr_1, ..., expr_n) : Unit
```

注意:

- 各引数式の型 `T_i` を保存する。
- 型名表示のため、`Ty -> String` 変換を共通化する。
- `dbg!` 自体は `Unit` 型なので、式位置・引数位置・binding 位置で通常通り扱える。

### 6. forge / codegen

対象例:

- `crates/forge/src/codegen.rs`

処理:

1. 各 `TypedDbgArg.expr` を左から順に emit する。
2. debug template を追加する。
3. `Opcode::Dbg { template_id, arg_count }` を emit する。
4. `Opcode::Dbg` の実行後に stack top が `Unit` になる前提で後続処理を続ける。

疑似コード:

```rust
for arg in args {
    emit_node(&arg.expr)?;
}
let template_id = add_dbg_template(args);
emit(Opcode::Dbg {
    template_id,
    arg_count: args.len() as u8,
});
```

### 7. sindr::ir / bytecode

対象例:

- `crates/sindr/src/ir.rs`

追加:

```rust
pub enum Opcode {
    // ...
    Dbg {
        template_id: u32,
        arg_count: u8,
    },
}

pub struct DbgTemplate {
    pub id: u32,
    pub span_start: u32,
    pub span_end: u32,
    pub args: Vec<DbgArgTemplate>,
}

pub struct DbgArgTemplate {
    pub ty: String,
    pub span_start: u32,
    pub span_end: u32,
}
```

`Bytecode` / `BytecodeChunk` に debug template table を追加する。

```rust
pub struct Bytecode {
    // ...
    pub dbg_templates: Vec<DbgTemplate>,
}

pub struct BytecodeChunk {
    // ...
    pub dbg_template_base: u32,
    pub dbg_templates: Vec<DbgTemplate>,
}
```

chunk 合成時には `template_id` の relocation が必要。

対象:

- `execute_chunk` 時の relocation
- `.eldr` encode/decode
- viewer metadata / dump 表示

### 8. eldr / VM

対象例:

- `crates/eldr/src/vm.rs`

`Opcode::Dbg` 実行処理:

```rust
Opcode::Dbg { template_id, arg_count } => {
    let values = pop_n(arg_count)?;
    let values = restore_source_order(values);
    let template = lookup_dbg_template(template_id)?;
    emit_dbg_report(template, values)?;
    stack.push(Value::Unit);
}
```

注意:

- stack から pop した値は逆順になるため、表示前に元の引数順へ戻す。
- `arg_count` と template args 数が一致しない場合は VM 実装不整合として runtime error。
- `inspect_value` 相当を使って値を表示する。
- 出力は stderr 側に流す。
- stdout / stderr capture policy に従う。

### 9. diagnostics

対象例:

- `crates/diagnostics/src/report.rs`
- `crates/diagnostics/src/render.rs`
- 必要なら `crates/diagnostics/src/debug.rs`

既存の `DiagnosticSpec` は error report に寄っているため、debug 専用 spec を追加するのが安全。

追加案:

```rust
pub struct DebugSpec {
    pub message: String,
    pub primary_span: Span,
    pub labels: Vec<DebugLabel>,
}

pub struct DebugLabel {
    pub span: Span,
    pub ty: String,
    pub rendered_value: String,
}
```

render:

- `ReportKind::Advice` など、error ではない種別を使う。
- 表示タイトルは `Debug: dbg!` とする。
- label message は `型名: inspect` とする。

### 10. xldr / REPL

対象例:

- `crates/xldr/src/repl/logic/render.rs`
- `crates/xldr/src/error_display.rs`

REPL 側に `dbg!` 専用表示ロジックは不要。
ただし VM が stderr に出す debug report は、既存の IO policy / capture policy に従って扱われる必要がある。

REPL の期待:

- `dbg!(x)` 単独実行では戻り値表示なし。
- `x = dbg!(y)` では binding 表示により `x: Unit = ()` が出る。
- `print(...)` と同じく、副作用出力後に入力待ちへ戻る。

### 11. dump / viewer

対象例:

- `crates/rune/src/commands/dump` 周辺
- `crates/sindr/src/viewer.rs`

追加対応:

- `Opcode::Dbg` を dump 表示できるようにする。
- debug template table を dump できるようにする。
- viewer 用 JSON に debug template を含めるか検討する。

最小実装では、opcode 表示だけでも可。
ただし `.eldr` に template を保存する場合、dump で確認できるほうが望ましい。

## 非公開性の要件

`dbg!` 実装で次をしてはいけない。

- `BUILTIN_METAS` に `dbg` / `dbg_template` を追加する。
- `BUILTIN_IMPLS` に `dbg` / `dbg_template` を追加する。
- stdlib `.srt` に `@builtin def dbg_template(...)` を追加する。
- `lib/bootstrap.srt` の `@intrinsic def dbg!...` を通常の callable builtin declaration として扱う。
- REPL completion symbols に `dbg_template` を出す。
- docs export に `dbg_template` を出す。

`dbg!` は構文としてはユーザが使えるが、内部 template 関数は存在しない。

## エラー方針

### parse error

```surtr
dbg! name
```

想定:

```text
Expected `(` after `dbg!`
```

### type error

`dbg!` 自体は任意型の式を受け取れるため、引数式内部で通常の型エラーが起きる。

```surtr
dbg!(1 + "x")
```

これは `dbg!` のエラーではなく、`1 + "x"` の型エラー。

### runtime error

引数式評価中に runtime error / Result error 表現が起きる場合は、通常の実行規則に従う。
`dbg!` は評価済みの値だけを表示する。

VM 内部不整合:

- template id が存在しない。
- `arg_count` と template args 数が一致しない。
- stack に必要な値がない。

これらは VM 実装ミスとして `RuntimeError` でよい。

## テスト観点

### lexer

- `dbg!(x)` が `Ident("dbg")`, `Bang`, `LParen`, `Ident("x")`, `RParen` として読める。
- `!=` が既存通り `BangEq` として読まれる。
- `!` 単体が `Bang` として読まれる。
- `dbg != x` が既存の比較式として壊れない。

### parser

- `dbg!(x)` が `Ast::Dbg` になる。
- `dbg!(x, y)` が複数引数の `Ast::Dbg` になる。
- `dbg!()` の扱いが仕様通りになる。
- `dbg(x)` は通常の `Ast::App` のまま。
- `dbg!(calc_score(hand))` の引数 span が `calc_score(hand)` を指す。
- `dbg!({|x| x})` の引数 span が closure 全体を指す。
- `dbg! name` が parse error になる。

### resolve

- `dbg!(x)` の `x` が通常通り resolve される。
- `dbg!` 自体は symbol table lookup されない。
- ユーザ定義 `def dbg(...)` が `dbg(...)` では呼べる。
- ユーザ定義 `def dbg(...)` があっても `dbg!(...)` は特殊構文として扱われる。
- `dbg_template` が名前解決できない。

### typecheck

- `dbg!(1)` の型が `Unit` になる。
- `dbg!("x")` の型が `Unit` になる。
- `dbg!(x, y)` の型が `Unit` になる。
- `x = dbg!(y)` で `x: Unit` になる。
- `foo(dbg!(x))` は `foo` が `Unit` を受け取る場合だけ通る。
- `dbg!(bad_type_expr)` は引数式側の型エラーになる。
- 各引数の型名が template に保存される。

### codegen

- `dbg!(a, b)` で `a`, `b` の評価 opcode が先に出る。
- その後に `Opcode::Dbg { arg_count: 2 }` が出る。
- `Opcode::Dbg` の後続式が `Unit` stack top 前提で処理できる。
- `x = dbg!(y)` で `StoreLocal` が `Unit` を保存する流れになる。
- `dbg!` の template id が正しく割り当てられる。
- chunk codegen で debug template base が正しく設定される。

### bytecode / relocation

- full program bytecode に debug template table が保存される。
- REPL chunk の debug template id が VM 側で正しく relocation される。
- `.eldr` encode/decode 後も `Opcode::Dbg` と template が保持される。
- `snapshot_bytecode` 後も template table が欠落しない。
- dump で `Opcode::Dbg` が確認できる。

### VM

- `dbg!(x)` 実行後、stack top が `Unit` になる。
- `dbg!(a, b)` の表示順が source order と一致する。
- `dbg!(side_effect_a(), side_effect_b())` の評価順が左から右である。
- `dbg!()` を許可する場合、表示なしで `Unit` を返す。
- `dbg!(x)` の debug 出力が stderr に出る。
- stdout capture と stderr capture が分離される。
- stderr capture 有効時、debug 出力が capture buffer に入る。
- `print(...)` の stdout 出力と `dbg!(...)` の stderr 出力が混ざらない。

### diagnostics / ariadne

- report title が error ではなく debug/advice 系になる。
- `Debug: dbg!` として表示される。
- `dbg!` 全体の span が primary span になる。
- 各引数の span に label が付く。
- label text が `型名: inspect` になる。
- 元の式文字列が本文に出ない。
- 関数コール式でも表示本文が長くならない。
- closure 式でも表示本文が長くならない。

### REPL

- `dbg!(x)` 単独実行で、debug 出力のみ出る。
- `dbg!(x)` 単独実行で、戻り値 `Unit` は表示されない。
- `x = dbg!(y)` で debug 出力が出る。
- `x = dbg!(y)` で `x: Unit = ()` が表示される。
- `print("x")` と同様、出力後に入力待ちへ戻る。
- REPL completion に `dbg_template` が出ない。
- REPL docs に `dbg_template` が出ない。

### 非公開性

- `dbg_template(...)` をユーザコードから呼べない。
- `:doc dbg_template` で見つからない。
- `:sig dbg_template` で見つからない。
- `BUILTIN_METAS` に debug template が存在しない。
- `BUILTIN_IMPLS` に debug template が存在しない。

### 回帰テスト

- 既存 `print` の stdout 動作が変わらない。
- 既存 `inspect` builtin の戻り値動作が変わらない。
- 既存 REPL binding 表示が変わらない。
- 既存 `Unit` top-level result 非表示が変わらない。
- 既存 `.eldr` decode が旧データに対して壊れないか確認する。
  - bytecode format version を上げる場合は migration / error message を整える。

## 実装順序案

1. lexer に `Bang` を追加する。
2. parser に `dbg!(...)` を追加する。
3. AST に `Dbg` を追加する。
4. resolve/typecheck を通す。
5. typed AST に `Dbg` を追加する。
6. `Ty -> String` helper を共通化する。
7. bytecode に `Opcode::Dbg` と `DbgTemplate` を追加する。
8. codegen で `Dbg` を emit する。
9. VM に `Opcode::Dbg` 実行処理を追加する。
10. diagnostics に debug report renderer を追加する。
11. REPL / CLI で stderr capture と表示を確認する。
12. dump / viewer に `Opcode::Dbg` 表示を追加する。
13. テストを追加する。

## 最終仕様まとめ

```text
dbg!(expr...)
= parser 特殊構文
= public builtin ではない
= 専用 Opcode で実行
= 各 expr を左から 1 回評価
= debug output に「型名: inspect」を表示
= 元 expr は ariadne の span/caption で示す
= 戻り値は常に Unit
= REPL では Unit top-level result は表示されない
= binding した場合は通常通り Unit binding として表示される
```
