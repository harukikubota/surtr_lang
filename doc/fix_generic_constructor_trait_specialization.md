# Generic TypeConstructor trait call specialization 修正仕様

## 目的

通常関数の direct TypeCtorTrait parameter / return が同じ family carrier を共有し、
その関数本体から親または同一 family の Trait method を呼ぶ場合に、call-site で得た
signature type substitution を specialization まで保持する。

対象例は次の `map_applicative` である。

```surtr
def map_applicative(
  value: Applicative<$A>,
  mapper: ($A -> $B)
) -> Applicative<$B> {
  Functor::fmap(value, mapper)
}

mapped: List<String> = map_applicative([9, 10], &to_string)
```

この呼び出しは `carrier = List`, `$A = Int`, `$B = String` と解け、
`Functor for List` を選択しなければならない。

## 現状と原因

call-site constraint solver は上の三つの置換を正しく生成している。一方、通常関数の
specialization mapping は `collect_bound_tyvars_for_def` が列挙した変数だけを
`Ty::UserFunc.call_substitution` から取り込む。

`map_applicative` では carrier witness と `$A` は列挙されるが、mapper の出力と
関数戻り値に現れる `$B` は列挙されない。このため具象化した本体が次の状態になる。

```text
receiver = List<Int>
arguments = [List<Int>, (Int -> unresolved $B)]
result = List<unresolved $B>
```

構造化された Trait method selector は receiver head だけでなく method の引数・戻り値も
同じ instantiation として照合するため、未解決 `$B` を待つ `Deferred` を返す。
specialization 境界はこれを `AmbiguousReturnTypeArgument` として拒否する。

回帰は `dfe4a2e1` で旧 receiver 中心の dispatch 選択を canonical method-role 全体の
構造選択へ置き換えた時点から発生する。旧経路を復活させてはならない。

## 変更後の契約

- call-site で解決済みの signature substitution のうち、specialized function 本体の
  pending Trait method instantiation が参照する変数を欠落させない。
- specialization key と body substitution は同じ必要変数集合を使う。追加の置換だけを
  body に適用し、key から落とす経路は設けない。
- Trait method selection は receiver、Trait arguments、ReturnTypeArguments、value
  parameters、return type を一つの canonical instantiation として照合する現行経路を保つ。
- carrier が具象でも、method instantiation に必要な型変数が call-site で本当に未解決なら
  `Deferred` を成功へ潰さず、既存の ambiguity 診断で拒否する。
- 候補数、宣言順、単一登録 carrier を型推論の証拠にするフォールバックは追加しない。

## 対象外

- TypeCtorTrait の構文・family・slot mapping の意味論変更
- `Functor` / `Applicative` / `Monad` の標準 API 変更
- 旧 dispatch 経路の復活
- 未解決 payload を任意の型として選ぶ既定値やフォールバック

## 受入条件

1. `lib/tests/list.srt` の `map_applicative([9, 10], &to_string)` が
   `List<String>` としてコンパイル・実行できる。
2. 同じ形を `Functor<$A> -> Functor<$B>` で書いた場合も成功し、親 Trait 継承の有無に
   依存した特例にならない。
3. 同一 generic function を異なる mapped output 型で呼んでも、specialization cache が
   別の method instantiation を誤再利用しない。
4. call-site、引数、期待戻り値のいずれからも必要な型を決められない既存の真の曖昧性は
   `AmbiguousReturnTypeArgument` のまま失敗する。
5. canonical structural Trait selection と impl obligation 検査を迂回しない。

## Level と検証

Trait method の canonical instantiation と通常関数 specialization のフェーズ間契約を
修正するため level 4 とする。

実装時は Scar の直接的な回帰テストを Red として追加し、少なくとも次を検証する。

```bash
rtk cargo nextest run -p scar
cargo run -- test --quiet list.srt
rtk cargo nextest run --profile ci --workspace
cargo run -- test --quiet --all
```

最終差分について独立レビューを行い、必要変数集合、specialization key、body substitution、
真の曖昧性の保持を確認する。
