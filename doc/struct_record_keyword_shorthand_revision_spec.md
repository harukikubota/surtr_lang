# Struct／Record の keyword・shorthand 修正仕様

本書は実装前の仕様である。今回の作業は本書の作成に限り、製品コード、実行可能テスト、正本文書は変更しない。後続の実装は本書単独で進められる。

## 目的・対象

Record の構築と構造的 Pattern で、keyword 指定がある場合に同名の変数・束縛名を shorthand として使えるようにする。keyword 指定のない形は、従来どおり宣言順の位置指定として扱う。型から位置やフィールド名を推測しない。

Struct は構造体リテラルに既にある shorthand を維持する。`Type(...)` から `new` を呼ぶ経路へ、この規則を広げない。

本変更は **level4** とする。Record の項目を keyword の有無で分類し、構築式と Pattern の正規化・型検査へ接続する新しい規則を定めるため、構文・型・フェーズ間の契約に影響する。

## 現行

- Record 構築は全項目を位置指定、または全項目を名前指定する。両者の混在とフィールド名 shorthand を拒否する。全フィールドの指定が必要で、名前指定の記述順は自由である。
- Record Pattern も全フィールドを位置指定、または全フィールドを名前指定する。混在とフィールド名 shorthand を拒否し、名前指定の照合・束縛は宣言順に行う。子 Pattern の total／partial によって通常 Bind `=` の可否を判定する。
- Struct リテラルの `Type { field }` は `Type { field: field }` の糖衣構文である。明示フィールドと混在できる。利用位置は所有型の既存規則で制限する。

現行の根拠は `docs/site/record.md`、`docs/site/structs.md`、`docs/dev/Pattern_spec.md` にある。実装では `crates/spire/src/parser/expr.rs` の項目解析、`crates/scar/src/checker/definitions.rs` の Record 構築、Pattern の名前解決・型検査を確認する。

## 変更後の項目分類

Record の一つの構築式、または一つの構造的 Pattern ごとに分類する。入れ子の子に keyword があっても、外側の分類には影響させない。

| その項目列の keyword 指定 | 分類 | keyword のない項目 |
|---|---|---|
| 一つもない | 位置指定 | 従来どおり宣言順に対応付ける |
| 一つ以上ある | 名前指定 | 構築では裸の変数、Pattern では裸の束縛名だけを shorthand として正規化する |

名前指定と判定した後で位置指定へ戻す経路は設けない。名前指定内の shorthand は位置引数ではなく、正規化後の名前付き項目である。

### Record 構築

名前指定では、裸の変数 `field` を `field: field` に正規化する。名前はフィールドを選び、値の変数は通常のレキシカルな名前解決で選ぶ。

keyword のない数値・文字列・呼出し・フィールドアクセス・演算などの式は、名前指定内では拒否する。値の型からフィールド名を推測しない。これらの式は `field: expr` と名前を明記すれば通常どおり使える。

正規化後は、宣言されたフィールド名・型・個数と照合する。未知名、重複、未指定フィールド、未束縛変数、値の型不一致を拒否する。shorthand は値の省略や既定値ではない。構築値の配置と評価は既存の Record 構築の宣言順を維持する。

```surtr
defrecord User(name: String, age: Int)
name = "Ada"
age = 37

a = User("Ada", 37)              # 位置指定
b = User(name, age)               # keyword がないので位置指定
c = User(name: "Ada", age)        # User(name: "Ada", age: age)
d = User(age: 37, name)            # User(age: 37, name: name)
e = User(name: name, age: age)     # 全項目を明示した名前指定
```

同じ型のフィールドでも、keyword のない裸の変数を名前指定へ読み替えない。

```surtr
defrecord Pair(first: Int, second: Int)
first = 1
second = 2
p = Pair(second, first)           # first に2、second に1を入れる
```

以下はそれぞれ独立した拒否例である。

```surtr
User(name: "Ada", 37)             # 名前指定内の数値に対応名がない
User(name: "Ada", age + 1)        # 名前指定内の任意式に対応名がない
User(name: "Ada", years)          # 未宣言のフィールド years
User(name: "Ada", age, age: 37)   # age の重複
User(name: "Ada")                 # age が不足
User(name: "Ada", age: "37")     # age の型不一致
```

### Record Pattern

名前指定では、裸の束縛名 `field` を `field: field` に正規化する。構築と違い、右側の `field` は既存変数の参照ではなく、新たな Pattern 束縛である。

名前指定内の keyword のない literal、`_`、型注釈付き Pattern、入れ子の構造的 Pattern、Extractor 等は shorthand として受理しない。これらの子 Pattern にはフィールド名を明記する。位置指定と判定された場合は、既存の子 Pattern の規則を維持する。

正規化後は全フィールドを宣言に対応付け、未知名、重複、不足、子 Pattern の型不一致、重複した束縛を拒否する。実際の子 Pattern の照合・束縛順はフィールド宣言順であり、ソース上の keyword の記述順には依存しない。total／partial の判定、通常 Bind `=` の条件、網羅性の範囲は変更しない。

```surtr
defrecord User(name: String, age: Int)
user = User("Ada", 37)

User(selected_name, selected_age) = user       # 位置指定
User(name: selected_name, age) = user          # age: age へ正規化
User(age: selected_age, name) = user           # name: name へ正規化

label = match user {
  User(name: "Ada", age) => age,
  User(name: _, age: _) => 0
}
```

以下はそれぞれ独立した拒否例である。

```surtr
User(name: "Ada", 37)             # 名前指定内の literal に対応名がない
User(name: selected_name, _)       # _ は裸の束縛名ではない
User(name: selected_name, years)   # 未宣言のフィールド years
User(name: selected_name, age, age: other_age)  # age の重複
User(name: selected_name)          # age が不足
User(name: 1, age)                 # name の子 Pattern が型不一致
```

## Struct の維持条件・対象外

- Struct リテラルの明示フィールドと shorthand の混在は、既存の成功・拒否条件を維持する。所有型、visibility、readonly、フィールド不足・重複・型不一致の規則を変更しない。
- Struct の `Type(...)`、`Type::new(...)`、一般関数呼出しには、Record の名前指定内の shorthand を追加しない。通常の位置引数・名前付き引数の混在拒否も維持する。
- Struct Pattern は既存の `deconstruct` 経路を維持する。
- Enum の宣言・構築・Pattern、constructor capture は対象外である。名前付き payload や新たな capture 構文を追加しない。
- OR Pattern の束縛整合性・比較規則は本変更の対象外とする。
- shorthand によるフィールド自体の省略、既定値、型によるフィールド名推測、任意式への名前推測を追加しない。

## 受入条件

1. 各 Record 項目列で keyword がゼロなら位置指定、一つ以上なら名前指定に確定する。入れ子の keyword は外側の分類を変えない。
2. Record 構築の名前指定内では裸の変数だけを同名フィールドへ正規化し、値を通常どおり名前解決・型検査する。裸の数値・任意式は拒否する。
3. Record Pattern の名前指定内では裸の束縛名だけを同名フィールドへ正規化し、literal・`_`・任意の子 Pattern を名前なしで受理しない。
4. 正規化後も全フィールドの名前・型・個数を検査し、未知・重複・不足と型不一致を拒否する。従来の全位置指定・全名前指定は引き続き使える。
5. Record の構築・Pattern の宣言順、Pattern の total／partial と既存 consumer の境界を維持する。
6. Struct リテラルの既存 shorthand と所有型の制約を維持し、一般関数、`new` 呼出し、Enum、constructor capture の受理範囲を広げない。
7. 置き換えた Record の混在拒否経路を残さない。拒否すべき入力を推測やフォールバックで救済しない。

## 単独の実装手順・正本更新

1. 実装開始時に本書と現行コード・既存テストを再照合する。`docs/site/record.md`、`docs/site/pattern-matching.md`、`docs/dev/Pattern_spec.md` の該当契約を先に整合させる。Struct の説明は適用範囲が明確になる最小限の追従とする。
2. Record 構築と Record Pattern の受入条件を直接検証する成功・拒否テストを追加・修正し、期待した契約の差で失敗することを確認する。既存の混在拒否 fixture は、受理する裸の変数・束縛名と、引き続き拒否する任意式・子 Pattern を分けて調整する。
3. Spire では項目列ごとの分類と shorthand の構造を保持する。Sigil で通常の変数参照または Pattern 束縛として名前解決し、Scar で宣言フィールドと対応付けて名前・型・個数を検査する。ソース位置を失わず、宣言・指定位置に基づく既存の診断契約を使う。
4. 後続フェーズへは、宣言フィールドとの名前による対応付けと宣言順に正規化した項目を渡す。構築値の配置・評価と子 Pattern の照合順を維持する。
5. 型から名前を推測する経路と旧分類へのフォールバックを残さず整理する。対象テストで Green を確認してから全体検証と独立レビューを行う。

正本の更新先は、上記に加え `docs/site/language-reference.md` の引数規約と `docs/dev/テスト方針.md` の関連項目とする。通常 call の混在禁止と Record の shorthand を区別して説明する。現行例を無関係な型や call へ一律に書き換えない。

## 検証方法

今回の仕様作成ではコンパイラのテストを実行しない。後続実装では、変更した契約を最も直接的な層で検証する。

- Spire の項目分類、Sigil の構築変数参照／Pattern 束縛、Scar の名前・型・個数の検査を、必要な crate-local テストに置く。
- Record の成功値と Pattern 束縛値、構築の評価順と子 Pattern の照合順を script fixture または既存の適切な実行テストで固定する。
- 任意式の推測拒否、未知・重複・不足・型不一致、一般関数や Enum の対象外境界を既存の拒否テストで確認する。Struct の既存 shorthand テストを維持する。
- 最小範囲から `rtk cargo nextest run -p spire`、`-p sigil`、`-p scar` の関連フィルタ、必要に応じて `rtk cargo nextest run -p rune --test integration run_srt` へ広げる。コマンドはリポジトリルートで実行する。
- level4 の最終検証は `rtk cargo nextest run --profile ci --workspace` と `cargo run -- test --quiet --all` とする。最終差分に独立した別エージェントレビューを実施する。実測結果、未実行範囲、残件を報告し、未実行を成功扱いしない。
