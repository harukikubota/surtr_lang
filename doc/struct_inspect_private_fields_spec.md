# 構造体の inspect と private フィールドの契約

## 状態と目的

本書は実装前の仕様案である。この PR は `doc/` の文書だけを追加し、コンパイラ、VM、標準定義、テスト、正本文書は変更しない。

構造体の内部状態をログで確認するためだけに `impl Struct` へ表示用メソッドを追加する必要をなくす。`inspect` は private フィールドも含む診断用の表示を提供する。`private` はフィールドの扱い方を型の定義側で管理するための境界とし、値の秘匿は保証しない。

`to_string` は `Show` による文字列変換として扱い、`inspect` を呼ぶ暗黙のフォールバックは削除する。

## 現行との関係

- `docs/site/structs.md` は、外側からのフィールドアクセスと Facet の作成を禁止し、`inspect` / `to_string` では private フィールドを `..private` に置き換えると説明している。
- `crates/eldr/src/builtin.rs` の `inspect_value` と `to_string_display` は `render_value` を共有する。構造体表示の `render_tagged_value` は `private_flags` を使って private フィールドを省略する。
- `lib/traits/Show.srt` は `Show::to_string` を文字列変換の契約として定義する。`docs/site/derive.md` も、`derive Show` が各要素に `Show` を要求し、`inspect` で暗黙に代用しないと定める。

既存の `Show` 要求は維持する。表示処理の共有だけを根拠に、すべての `to_string` が現在 `inspect` にフォールバックしているとは扱わない。実装時に呼び出し経路を確認し、暗黙の代用があれば削除する。

## private の意味

`private` は、所有者の `impl Struct` / `impl Trait for Struct` の外から、フィールドへの直接アクセスや、そのフィールドを扱う Facet を導出することを禁止する。

Facet は、定義から外側で呼び出せる get / set 操作を導出する仕組みである。private フィールドについては、その導出を外側に許可しない。`inspect` がフィールド名と内容を表示しても、この規則は変わらない。

フィールドの値を外へ持ち出すこと自体は、元々禁止していない。型の定義側がメソッドで値を返したり、許可する操作を公開したりすることはできる。`private` の目的は、その扱い方を定義側で管理することである。

`inspect` の文字列を解析して値を取得することも禁止しない。ただし、文字列から元のフィールド参照や Facet を取得することはできず、元の構造体のフィールドを書き換える権限も得られない。

`private` は情報の秘匿を保証しない。たとえば private な password の内容も `inspect` に現れる。状態への操作を制限することと、ログへの情報の露出を防ぐことは別の契約である。

## inspect の表示

- 構造体の全フィールドを定義順に表示する。private フィールドも省略せず、`..private` は出力しない。
- フィールド名と値の表示形式は既存の `Type(field: value, ...)` を維持する。
- フィールド値は再帰的に `inspect` の表示規則に従う。List、Tuple、Result などの中にある構造体にも同じ規則を適用する。
- String の引用・エスケープ規則は維持する。
- 呼び出し側のスコープや `Show` の有無によって表示内容を変えない。`Show` を自動で呼び出す経路は設けない。
- 戻り値は診断用の String であり、フィールドへの操作権限を含まない。

表示は再入力可能なコードであることを保証しない。構造体の構築は `new` または型の定義側が提供する構築経路に従うため、表示されたフィールド列がコンストラクタの引数と一致する必要はない。private フィールドを表示することによって、構築や分解の契約を変更しない。

## to_string と Show

`to_string` は対象型の `Show` 契約に従う。`Show` が必要な箇所で実装がなければ型検査で拒否し、`inspect` による暗黙の代用は行わない。型検査を通過した後の不正な内部状態も、汎用表示へのフォールバックで隠さない。

標準型の明示的な `Show` 実装と、その契約を保つ最適化は維持する。`derive Show` は各フィールド・payload の `Show` を要求する既存の契約を維持する。本仕様の全フィールド表示を、`to_string` 全般へ自動適用しない。

型の定義側が手書きの `Show::to_string` で明示的に `inspect(self)` を呼ぶことは許可する。これは作者が選択した表示であり、削除対象の暗黙のフォールバックには含めない。

## 成功例と拒否例

次は変更後の契約を示す例であり、この PR では実行・実装しない。

```surtr
defstruct User {
  name: String,
  private password: String,
}

impl User {
  def new(name: String, password: String) -> Self {
    User { name, password }
  }
}

user = User("alice", "secret")
print(inspect(user))
# User(name: "alice", password: "secret")
```

外側スコープでは、以下を引き続き拒否する。

```surtr
print(user.password)  # private フィールドへの直接アクセス
User.password         # private フィールドの Facet の導出
```

この `User` には `Show` を定義していないため、`to_string(user)` は拒否する。`inspect(user)` は成功する。

所有者の `impl` 内で `self.password` を読み、メソッドから String として返すことは許可する。表示された String の解析結果も通常の値として使えるが、元のフィールド参照としては扱わない。

## 受入条件と実装時の検証

1. private フィールドを持つ構造体の `inspect` が、フィールド名と値を定義順に表示する。全フィールドが private の場合も省略しない。
2. 入れ子の構造体と各コンテナでも同じ表示規則を使い、String の引用・エスケープを維持する。
3. 外側の直接アクセス、private Facet の作成、およびそれを通じた get / set は引き続き拒否する。所有者側で値を取り出して外へ返す成功例は維持する。
4. `Show` のない型の `to_string` は拒否する。標準型の明示的な `Show`、`derive Show`、手書きの `Show` はそれぞれの契約で動作する。
5. 手書きの `Show` が明示的に `inspect` を呼ぶ成功例を確認する。
6. `to_string` から `inspect` への暗黙のフォールバックを残さない。旧仕様の省略期待値は新しい表示に合わせて更新する。
7. フィールドの可視性情報はアクセス検査に必要なため保持し、表示時の省略判定だけを取り除く。スコープ別の表示分岐や互換経路を追加しない。

実装は可視性と観測の契約変更として level4 とする。表示処理、型検査の成功・拒否境界、標準の Show 実装、REPL と診断表示への波及を確認する。最終差分は別エージェントがレビューし、`rtk cargo nextest run --profile ci --workspace` と `cargo run -- test --quiet --all` を実行する。

実装時には `docs/site/structs.md`、`docs/site/kernel.md`、`docs/site/strings.md`、`docs/site/derive.md`、`lib/Kernel.srt` の `@doc`、および関連する `docs/dev/` の表示契約を整合させる。本 PR では、未実装の内容を正本文書へ反映しない。
