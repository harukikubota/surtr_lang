# b1fb39c4 後の Scar コンパイル時間回復仕様

## 目的

`b1fb39c4e64841713ad6938ecbc3133fbde87642` の次のコミットから現在の
HEAD までに入ったコンパイラ変更を対象に、Surtr ソースの cold compile
時間を短縮する。`b1fb39c4` 自体のテスト実行時間短縮は対象に含めない。

この変更は型検査の意味論や診断内容を変えず、Scar が投機的な型互換性確認の
ために保存する状態を、その確認が変更し得る範囲へ限定する。

## level と影響範囲

- level 3
- 主対象: `crates/scar/src/checker/matching.rs`
- 共通化が必要な場合の対象: `crates/scar/src/checker/relations.rs`
- 回帰テスト: Scar の crate-local test と既存の構造化診断・return type argument test
- 手動性能測定: `tests/profile/heavy_compile.srt`

型関係の推論状態を扱い、`match` の成功・失敗判定へ影響し得るため level 3 とする。
言語仕様、構文、型規則、診断 schema、runtime、fixture runner は変更しない。

## 調査結果

### コミット境界

同じ debug build、空の stdlib cache、`SURTR_SCAR_PROFILE=1`、
`tests/profile/heavy_compile.srt` で測定した代表値は次のとおり。

| revision | cold wall time | stdlib Scar total | isolated body | match arm |
|---|---:|---:|---:|---:|
| `b1fb39c4` | 2.37 s | 595.793 ms | 171.590 ms | 49.211 ms |
| `c28d185c` | - | 610.792 ms | 173.917 ms | 50.783 ms |
| `7dc8e0d1` | - | 601.908 ms | 169.747 ms | 49.484 ms |
| `c027048b`（`1223cc37` の直前） | - | 621.078 ms | 171.408 ms | 49.487 ms |
| `1223cc37` | 5.11 s | 3083.943 ms | 2467.162 ms | 797.606 ms |
| `85465d17`（調査時 HEAD） | 3.23 s | 1349.134 ms | 870.327 ms | 138.551 ms |

`c027048b` までは測定揺らぎの範囲だが、構造化シグネチャ診断を統合した
`1223cc37` で stdlib の Scar 時間が約 5 倍になった。`85465d17` の型関係専用
チェックポイントにより約 56% 回復したものの、直前値の約 2.17 倍が残る。

調査時 HEAD のプロセスサンプルでは、標準定義の Trait impl body 内の
`check_match` から `candidate_probe_checkpoint` に入るスタックが支配的だった。
代表スタックでは `check_match` の 208 samples 中 184 samples が同チェックポイント
内にあり、別の分岐でも 76 samples 中 65 samples を占めた。

### 根因

`check_match` は二つ目以降の arm ごとに、通常の arm 型関係診断とは別に
Err-only self arm coercion の可否を投機的に確認する。この確認は終了時に必ず
状態を戻すが、現在は候補探索用の `candidate_probe_checkpoint` を使う。

候補探索用チェックポイントは次を全体複製する。

- `TypeEnv`
- substitutions、型変数 bound、pending Trait obligation
- active / constructor capability と constructor witness の各 map
- warning buffer

この箇所が実行するのは `types_compatible` と、同関数を用いる
`can_coerce_err_only_result_self_arm` だけである。`types_compatible` が直接更新する
推論状態は substitutions、型変数 bound、pending Trait obligation であり、
候補の式を型検査するための `TypeEnv`、capability、warning の保存は不要である。
arm 数に比例して大きな `TypeEnv` を複製するため、標準定義の isolated body と
match arm の時間が増えている。

`carriers.rs::unify_constructor_carriers` と `expr.rs` にも候補探索用チェックポイントは
あるが、前者は `9d435d2a` の時点ですでに存在し、`c027048b` までの測定では急増を
起こしていない。後者の callable-shape probe は `b1fb39c4` より前から存在する。
また、これらは成功時の束縛を commit する経路を含むため、常時 rollback する
`match` の probe と同一変更にまとめない。

## 変更後の契約

### 1. `match` coercion probe の状態境界

Err-only self arm coercion の確認は、比較対象に含まれる変更可能な型変数について
次だけを保存する。

- substitutions
- 型変数 bound
- pending Trait obligation

比較対象は少なくとも、既存結果型、現在 arm の body 型、scrutinee 型を含める。
既存の型関係チェックポイントと同様に、解決先の型変数と constructor family
witness root まで追跡する。

probe の成否にかかわらず、確認の終了時に保存した推論状態を復元する。
concrete 型と rigid 型だけの比較では、保存対象を作らない。

### 2. 既存挙動の維持

- Err-only self arm coercion を許可する条件と、coercion 後の arm body 型を変えない。
- 通常の arm 型関係チェックは成功時の推論束縛を保持し、失敗時だけ部分束縛を戻す。
- 一つ目の不一致を記録しつつ残りの arm を検査する構造化診断の収集を変えない。
- diagnostic reason、origin、primary / related source fact、branch context を変えない。
- 候補探索中に式、環境、capability、warning を変更し得る既存経路は、引き続き
  `candidate_probe_checkpoint` を使う。
- 新しいフォールバックや、失敗を成功として扱う経路を追加しない。

### 3. 実装境界

`relations.rs` の既存 `TypeRelationCheckpoint` を、`matching.rs` から安全に利用できる
最小の API にする。用途の違いを呼び出し側で明示する。

- type relation assertion: 成功時 commit、失敗時 rollback
- match coercion probe: 成功・失敗の双方で rollback

候補探索用チェックポイント自体を縮小しない。式の型検査を行う本来の候補探索と、
型互換性だけを見る probe の責務を混ぜない。

## 実装段階

### 段階 1: 軽量 probe

1. `TypeRelationCheckpoint` の保存対象収集を、複数の `Ty` から呼べる形に限定公開する。
2. `check_match` の Err-only self arm coercion を、その軽量チェックポイントで囲む。
3. 成否にかかわらず rollback することをコード上で一意にする。
4. `candidate_probe_checkpoint` の他の呼び出し箇所は変更しない。

### 段階 2: 再測定と限定監査

段階 1 後に同条件で再測定する。受入基準を満たさない場合だけ新しいサンプルを取り、
`check_match_arm` の arm-local `constructor_capabilities` 全体 clone を次の候補として
調査する。根拠なしに `carriers.rs` や `expr.rs` の checkpoint を一括置換しない。

## テスト

### crate-local 契約

- Err-only self arm coercion probe が、途中で作った型変数束縛、bound、pending
  obligation を成功・失敗のどちらでも残さない。
- probe を通った既存の成功例
  `expected_generic_result_allows_err_only_self_match_arm` が成功する。
- match arm 不一致が従来と同じ `MatchArmTypeMismatch` と source facts を返す。
- match coercion probe が `candidate_probe_checkpoint` を作らないことを、既存の
  test-only counter で固定する。
- 式を実際に型検査する candidate probe が全状態 rollback を維持する既存テストを通す。

最初の検証:

```bash
rtk cargo nextest run -p scar expected_generic_result_allows_err_only_self_match_arm
rtk cargo nextest run -p scar branches_keep_reason_and_source_facts
rtk cargo nextest run -p scar
```

Scar 内部の局所的な性能修正なので、対象が Green なら workspace 全体の検証は必須と
しない。変更が fixture の診断出力へ及んだ場合だけ、次を追加する。

```bash
rtk cargo nextest run -p rune --test integration run_srt
```

### 性能測定

各 revision で `cargo build -p rune` を一度行い、各試行で新しい一時 directory を
作って stdlib cache と出力を分離する。

```bash
bench_dir=$(mktemp -d)
/usr/bin/time -p env \
  SURTR_STDLIB_CACHE_DIR="$bench_dir/cache" \
  SURTR_SCAR_PROFILE=1 \
  ./target/debug/surtr build \
  tests/profile/heavy_compile.srt \
  "$bench_dir/heavy.eldr"
```

`c027048b`、変更直前 HEAD、変更後を同じマシン・toolchain で各 5 回測定し、
stdlib の Scar total と wall time の中央値を比較する。Rust build 時間は含めない。
hot compile も 1 回確認し、cache 利用時の退行がないことを確認する。

## 受入条件

- 上記 crate-local 契約がすべて Green である。
- `check_match` の Err-only self arm coercion から、全状態を複製する
  `candidate_probe_checkpoint` 呼び出しがなくなる。
- 変更後の cold stdlib Scar total の中央値が、同条件の変更直前 HEAD より 25% 以上
  短く、かつ `c027048b` の 1.5 倍以内になる。
- hot compile の中央値が変更直前 HEAD より 10% を超えて悪化しない。
- 型検査結果、構造化診断、warning、生成 bytecode を変更しない。
- 性能条件を満たさない場合は完了扱いにせず、段階 2 の再サンプル結果と残る
  hotspot を記録する。

参考目標として、cold stdlib Scar total が `c027048b` の 1.25 倍以内まで戻ることを
目指すが、これは必須条件にはしない。

## 対象外

- `b1fb39c4` の fixture runner / test cache 最適化
- Rust のビルド時間短縮
- 言語仕様、型規則、診断 schema の変更
- candidate selection の回数や候補順序の変更
- full candidate checkpoint 全体の再設計
- 計測根拠のない一括キャッシュ、遅延化、並列化
