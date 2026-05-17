# Compiler Warnings

Surtr の警告は、コンパイルを止めない軽量な指摘です。  
エラーとは違い、プログラムの意味が確定できる場合に「おそらく意図と違う」「後から読みづらくなる」箇所を示します。

Phase1 では警告を compiler 内部の buffer に集めるところまでを対象にしています。  
`surtr check` / `run` / `build` / `test` / REPL の表示、JSON 出力、`--deny-warnings` のような扱いはまだ接続していません。

## 対象

Phase1 で扱う警告は次の 4 種類です。

| warning | phase | 意味 |
|---|---|---|
| `UnusedVariable` | resolve | 束縛した変数が使われていない |
| `UnusedImportFunction` | resolve | 明示 import した関数を短い名前で使っていない |
| `UnusedValue` | typecheck | `Unit` ではない値を文の途中で捨てている |
| `UnusedTypeParameter` | typecheck | 宣言した型引数が実際の型位置で使われていない |

## Unused Variable

ローカル束縛、関数引数、closure 引数、match pattern の束縛、`as` alias などを定義して、本文で参照しない場合に警告します。

```surtr
def greeting(name: String, unused: String) -> String {
  "hello #{name}"
}
```

意図的に値を使わない場合は `_` を使います。

```surtr
def greeting(name: String, _: String) -> String {
  "hello #{name}"
}
```

`_name` は特別扱いしません。未使用を明示したい場合は `_` を使ってください。

## Unused Import Function

`import Mod::f` や `import Mod::{f}` のような明示的な関数 import が、短い名前で使われていない場合に警告します。

```surtr
import Math::double

def main() -> Int {
  1
}
```

短い名前で呼ぶと使用済みになります。

```surtr
import Math::double

def main() -> Int {
  double(21)
}
```

qualified call は明示 import を消費しません。

```surtr
import Math::double

def main() -> Int {
  Math::double(21)
}
```

この形では `double` を unqualified name として使っていないため、import は不要です。

`import Mod` の unused member 検出は Phase1 の対象外です。auto import もこの警告の対象外です。

## Unused Value

block や top-level の途中にある文が `Unit` ではない値を返し、その値を使っていない場合に警告します。

```surtr
def main() -> Unit {
  1
  ()
}
```

意図的に捨てる場合は明示的に `;` を付けます。

```surtr
def main() -> Unit {
  1;
  ()
}
```

`Unit` を返す文は警告しません。

```surtr
def main() -> Unit {
  print("hello")
  ()
}
```

`Result<Unit>` は `Unit` ではないため、途中で捨てると警告します。

```surtr
def main() -> Unit {
  Ok(())
  ()
}
```

この場合も、意図的に捨てるなら `Ok(());` と書きます。

## Unused Type Parameter

関数、extractor、struct、enum、trait、trait method、trait impl method の型引数が、引数型・戻り値型・field・variant payload・関連 method signature などの実際の型位置に現れない場合に警告します。

```surtr
def id<$A, $Unused>(value: $A) -> $A {
  value
}
```

`$A` は引数型と戻り値型で使われていますが、`$Unused` は使われていません。

型引数の bound だけでは使用とはみなしません。

```surtr
deftrait Describe<$A: Show> {
  def describe() -> String
}
```

この例では `$A` が method signature に現れないため、bound があっても未使用として扱います。  
Surtr の trait は単純方向の解決機構として扱うため、PhantomType 的な逃げ道は用意しません。

## 現時点の扱い

- 警告は compiler phase が `WarningBuffer` に積みます。
- 既存の compile API は警告を破棄し、従来の挙動を保ちます。
- `_with_warnings` 系 API は warning list を含む `PhaseOutput<T>` を返します。
- CLI 表示、JSON `warnings`、警告抑制属性、deny warnings は Phase1 では未対応です。

## 関連ページ

- `;` と `Unit` の基本は `./language-reference.md`
- import / auto import は `./language-features.md`
- trait と型引数は `./trait-impls.md`
