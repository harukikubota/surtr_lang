# `chain` の衝突と callable 同一性: 設計判断

## 既存契約と現状

`doc/要件定義v9.md` は auto-import 同士の同名 unqualified 関数を compile error とし、`Result::chain` と `Facet::chain` を異なる公開 API と定めている。現状は `lib/types/result.srt` と `lib/facet.srt` の両 `impl` が `@autoimport` で、Sigil はこの2関数の bare `chain` 衝突を特例で許す。Scar は `Result::chain` の解決済み呼び出しでも引数形から `Facet::chain` を試し、`Result::chain(pair._0, Ok(()))` を Facet 型エラーにする。

## 衝突を解消する際の設計判断

`@autoimport` は impl block 全体に適用される。impl member 個別への `@autoimport` は parser が拒否し、同じ型の複数 inherent `impl` block も Sigil が拒否する。このため既存の仕組みでは `Facet::chain` だけを auto-import 対象から外し、他の bare Facet helper を維持できない。

候補は次のとおり。

1. **Facet 全体の auto-import を外す。** `Facet::chain` と `/` を明示する設計には合うが、bare `put` なども使えなくなる。現行の `facet_shorthand.srt` と Scar の surface tests は bare `put` を使い、bulk-update の `set` / `over` / `case_over` も同じ auto-import に依存する可能性がある。これらを含む公開 surface の変更を正本と一緒に決める必要がある。
2. **Result 全体の auto-import を外す。** bare `chain` に加え、`is_ok` / `recover` など他の標準 helper も影響を受ける。標準テストと既存利用者への影響が大きい。
3. **member 単位の auto-import opt-out を設計する。** 他の bare helper を保てるが、新しい annotation または同等の宣言規則が必要で、現行の単純な block 単位ルールを拡張する。

どの候補を採るかは監査項目の「衝突を拒否する」という目標だけでは確定できない。選択後に `doc/要件定義v9.md` を先に整合させ、公開 surface の成功・拒否境界を確定する。現行の衝突特例だけを削ると、標準 `Result` と `Facet` を読み込む通常スクリプト全体が失敗する。

## 独立して確定している修正

Scar は明示 `Result::chain` を引数形から Facet として再解釈しない。`Facet::chain` は Facet 合成として扱う。qualified 呼び出しに限る修正は level 3 として Scar surface test と Result script fixture で TDD 済み。bare `chain` の既存特例は auto-import 方針が決まるまで維持する。

追加で次の明示 import が未解決である。実測では `Expected Facet<...> value, got Result<Int>` が `pair._0` に出る。

```surtr
import Result::chain;
pair: (Result<Int>, Int) = (Ok(7), 1)
print(inspect(chain(pair._0, Ok(()))))
```

Sigil の `ModuleScopeBuild` は `explicit_function_imports` と `effective_auto_import_fq_names` を持つが、`Scope` は名前から UID への対応のみを保持する。`ResolvedId` に import 由来はなく、`Ast::Import` も resolved program には残らない。このため Scar に届く explicit bare import と auto-import の `chain` は、同じ `name=chain`、`qualified_name=Result::chain`、UID になり、現在の情報では区別できない。

この境界を修正するには、Sigil の binding origin を `ResolvedId` まで保持し、Scar へ渡す phase 間契約を定める必要がある。受入条件は、上の明示 import 例が `Ok(7)` を出すこと、qualified `Result::chain` が Result として動くこと、既存 bare Facet / Result `chain` の契約を意図せず変えないこと。由来を判定できないときに引数形から owner を推測して成功へ変える経路は追加しない。

残る auto-import 衝突の解消は level 4。標準 surface と Sigil→Scar の解決済み callable 契約に及ぶ。設計決定後に Sigil の衝突拒否、既存 bare helper の成功・拒否境界を TDD で固定し、`rtk cargo nextest run --profile ci --workspace` と `cargo run -- test --quiet --all` を実行する。最終差分に対する独立レビューも必要。
