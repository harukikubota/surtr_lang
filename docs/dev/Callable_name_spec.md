# 関数名の `?` suffix

関数名の末尾には、隣接する `?` を1個付けられる。一般の識別子規則は変更せず、関数名を読む位置にだけ適用する。

## 構文とフェーズの責務

Spire は `def` / `defp`、trait / impl / builtin 宣言、通常・修飾 call、capture、import、backtick call の最終関数名に suffix を含める。`predicate?::<T>()` の返り型引数も既存の文法に従う。値引数との型入力の重複や、返り型に現れない返り型引数は従来どおり拒否する。suffix を理由に返り型引数の制約を緩めない。Sigil は suffix を含む名前をそのまま解決し、suffix のない名前への fallback を行わない。

文頭の通常・修飾 suffix call は Pattern の試行対象から外す。`predicate?() = rhs` は不正な LHS として拒否する。Extractor 名には suffix を許可しないが、Pattern の事前引数など既存の Expr 位置では suffix call を許可する。Pattern 内に `?` があることだけで解析を Expr へ切り替えない。

変数、引数、Pattern の束縛、フィールド、型、モジュールの名前には suffix を許可しない。裸の関数名は従来どおり関数値にならず、`&predicate?` で capture する。予約 consumer の bare capture 禁止と Pattern 引数の契約も維持する。

## 返り型

Scar は解決・正規化した関数署名の返り型が canonical Boolean であることを確認する。引数だけが generic な関数を許可する。現行の `type` 宣言は関数型 alias に限定され、Boolean 自体の alias は宣言できない。通常関数、trait 宣言・実装、inherent impl、builtin 宣言に同じ制約を適用する。

`Result<Boolean>`、`Option<Boolean>`、Boolean を返す関数値、未確定の返り型は拒否する。呼出し側で Boolean に具体化できることは宣言を受理する理由にならない。既存の返り型省略・builtin 署名規則は変更しない。suffix のない Boolean 関数は引き続き宣言できる。

## 他の `?` との境界

| 入力 | 扱い |
|---|---|
| `predicate?(1)` | suffix 関数の call |
| `&predicate?` | suffix 関数の capture |
| `predicate?(1)?` | call と文末アンラップ。Boolean のアンラップは型エラー |
| `ret?` / `Identity ?` | 既存の文末アンラップ |
| `Int?` | 型位置で `Option<Int>` |
| `value . Identity ?` | 廃止済み OptionalSelector として構文エラー |

名前解決や型検査の結果によって構文を解析し直さない。文末アンラップの位置・対象型・返却先・評価規則は変更しない。OptionalSelector の受理経路は設けない。

補完、tolerant parser、REPL の署名・文書照会も suffix を含む表記を扱う。スキーマバージョンと VM バージョンは変更しない。

Tuple の番号 selector（`tuple._0?` など）の末尾の `?` は、従来どおり文末アンラップとして扱う。番号 selector は廃止済み OptionalSelector の対象には含めない。
