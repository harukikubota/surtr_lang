# Precompiled Stdlib Plan

## 目的

- Rust 側の clean build 短縮と、Surtr source compile の短縮を分けて扱う
- Surtr source compile では、毎回実行している標準 library の parse / resolve / typecheck を短縮する
- `.eldr` は実行 bytecode の正本として維持し、compile-time context の precompile は別形式として策定する

## 作成日

- 2026-04-29 (Asia/Tokyo)

## 現状

Rust clean build の baseline は `006_compile_time_reduction_plan.md` の feature 調整後を採用する。
追加の clean build 短縮では、依存 crate を増やさず、既存依存の feature / crate boundary / test 配置を優先して見直す。

Surtr source compile では、ユーザー source よりも標準定義ソース群の初期化が支配的になっている。
`tests/profile/heavy_compile.srt` で確認した値は次の通り。

```bash
SURTR_SCAR_PROFILE=1 /usr/bin/time -p ./target/debug/surtr build tests/profile/heavy_compile.srt /tmp/surtr-heavy-plan.eldr
```

| item | value |
|---|---:|
| `surtr build` real | 1.25s |
| `surtr build` user | 0.50s |
| Scar total | 372.431ms |
| Scar stmt count | 405 |
| Scar check loop | 365.474ms |
| slow kinds | `Def`: 290.711ms, `TraitImplDef`: 42.401ms, `DeferrorDef`: 21.778ms |

読み取り:

- `compile_source` は default stdlib を含む module stages を毎回 parse / precollect / resolve / typecheck している
- heavy fixture 自体は小さく、Scar の遅さは主に `lib/*.srt` と trait modules に由来する
- precompile の主対象は user program ではなく、固定の標準 library semantic context である

## 方針

### 1. Rust clean build

Rust clean build は `006_compile_time_reduction_plan.md` を baseline とし、次の順で継続する。

1. 既存依存の feature を絞る
2. test-only の重い helper / module を integration test へ逃がす
3. crate boundary を見直し、編集時の再コンパイル波及を小さくする

precompiled stdlib のために新しい runtime dependency を増やすことは避ける。
disk cache v2 で serialization が必要な場合も、既存の `serde` / bytecode 周辺依存を優先して使う。

### 2. v1: process-local semantic snapshot

v1 は disk cache を作らず、プロセス内の `OnceLock` snapshot として実装する。
目的は CLI / test process 内で同じ標準 library 初期化を繰り返さないことである。

snapshot は `xldr` 側に集約し、次の情報を持つ。

- default stdlib の parse 済み `module_stages`
- `sigil::DeclarationIndex`
- REPL / script 用に再利用できる Sigil scope 相当
- Scar session checkpoint 相当の型環境
- stdlib doc metadata

`rune::compile_source` は default stdlib だけを再処理せず、snapshot を基準に user source と include module を後段として処理する。
追加 std module、`include`、project module は snapshot に混ぜ込まず、従来通り parse / resolve / typecheck する。

実装時の境界:

- `xldr` に `default_stdlib_semantic_snapshot()` のような helper を追加する
- `SigilSession` / `ScarSession` には、snapshot から安全に復元する public API を最小追加する
- snapshot は標準 library 専用とし、user source や project module を保存しない
- snapshot 作成エラーは既存の `LoadError` / phase diagnostic に落とす

v1 の期待効果:

- `surtr build` / `surtr run <file.srt>` の warm process 内繰り返しで標準 library 型検査を省く
- `rune` integration tests の同一 process bucket で効果が出る
- disk invalidation を持たないため、実装リスクを小さく保てる

### 3. v2: disk semantic cache

v2 は v1 snapshot を安定 DTO 化し、disk cache として保存する。
cache file は次に固定する。

```txt
target/surtr-stdlib-cache/std.semantic
```

cache key は次の値から作る。

- semantic snapshot schema version
- compiler version
- `BUILTIN_METAS` content hash
- `BUILTIN_TYPE_METAS` content hash
- default `lib/*.srt` content hash
- bytecode / type context に影響する compile policy version

cache miss、corrupt cache、version mismatch、hash mismatch は silent rebuild とする。
cache が壊れていてもユーザー visible error にせず、通常の source compile へ戻す。

disk snapshot は Rust private struct をそのまま永続化しない。
`serde` 可能な安定 DTO を定義し、復元時に `SigilSession` / `ScarSession` / doc metadata へ変換する。

### 4. `.eldr` との関係

`.eldr` は実行入力であり、引き続き次の実行用情報を保持する。

- `Code`
- `Cnst`
- `Func`
- `Type`
- `ErrT`
- `CInf`
- viewer / diagnostics 用 chunk
- optional `Docs`

compile-time snapshot は `.eldr` へ統合しない。
`.eldr` に Sigil / Scar の内部 context を追加すると、実行 artifact と compiler cache の責務が混ざるためである。

REPL の `.eldr` load は現状、標準 library の compile-time context を source から復元している。
v2 以降では semantic snapshot を使い、この復元コストと「user-defined function は名前解決対象として復元されない」制限の緩和を別タスクで扱う。

## 実装手順

### v1

1. `xldr` に default stdlib snapshot builder を追加する
2. builder 内で default stdlib の module collection / parse / declaration precollect / resolve / Scar typecheck を一度だけ行う
3. `SigilSession` と `ScarSession` に snapshot 復元 API を追加する
4. `rune::compile_source` を snapshot 経由へ切り替える
5. 追加 std module / include module がある場合は snapshot 後段 stage として従来処理する
6. `SURTR_SCAR_PROFILE=1` で snapshot on/off の差分を測定して、この doc に追記する

### v2

1. snapshot DTO と schema version を定義する
2. cache key を計算する helper を追加する
3. `target/surtr-stdlib-cache/std.semantic` の read / validate / write を追加する
4. corrupt / old cache の fallback test を追加する
5. REPL `.eldr` load で semantic cache を使うか検討する

## テスト計画

計測:

```bash
CARGO_TARGET_DIR=/tmp/surtr-clean cargo build --workspace --timings
SURTR_SCAR_PROFILE=1 ./target/debug/surtr build tests/profile/heavy_compile.srt /tmp/surtr-heavy.eldr
```

v1 実装後は snapshot off / on の `surtr build` wall-clock と Scar profile を比較する。

回帰:

```bash
cargo nextest run --workspace
cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket_0
```

追加するテスト観点:

- default stdlib snapshot が通常 compile と同じ typed / bytecode 結果を生む
- include module は snapshot 後段で通常通り参照できる
- 追加 std module は default snapshot に吸収されない
- stdlib source hash が変わると v2 cache が invalidation される
- corrupt cache / old schema cache は silent rebuild される
- `.eldr` decode / run は semantic snapshot の有無に影響されない

## 受け入れ条件

- `.eldr` format と実行互換性を変えない
- `lib/*.srt` と `BUILTIN_METAS` / `BUILTIN_TYPE_METAS` が標準 library 意味論の正本であり続ける
- snapshot は派生物であり、正本 source と矛盾した場合は破棄して再構築する
- v1 は process-local cache のみで完了可能にする
- v2 disk cache は失敗しても通常 compile へ fallback する

## v1 実装後メモ

- 実装日: 2026-04-29
- 実装範囲: process-local `OnceLock` snapshot のみ。disk cache は未実装。
- snapshot は default stdlib の parsed module stages / declaration index / Sigil resume state / Scar checkpoint / precompiled bytecode / doc metadata を保持する。
- `rune::compile_source` は default stdlib stage を snapshot から復元し、追加 std module / include / user source だけを後段として resolve / typecheck / codegen する。
- `.eldr` 形式は変更しない。precompiled stdlib bytecode の top-level `Halt` 手前へ user chunk top-level を合成し、関数 body と PC を再配置する。

計測:

```bash
SURTR_SCAR_PROFILE=1 /usr/bin/time -p ./target/debug/surtr build tests/profile/heavy_compile.srt /tmp/surtr-heavy.eldr
```

| item | value |
|---|---:|
| `surtr build` real | 0.58s |
| `surtr build` user | 0.53s |
| cold snapshot Scar total | 369.055ms |
| post-stdlib Scar total | 26.167ms |
| post-stdlib stmt count | 39 |

読み取り:

- 単発 CLI process では snapshot 初期化が cold cost として一度発生する。
- 同一 process 内の warm compile では default stdlib の Scar 処理は省かれ、後段 user/include 分だけが対象になる。
- この測定では post-stdlib の Scar 対象は 405 stmt 相当から 39 stmt へ縮小した。

## v1.5 / v2 branch 実装メモ（未採用）

- 実装日: 2026-04-29
- 実装範囲: `xldr` の default stdlib semantic snapshot を disk cache 化する branch を作成した。
- この時点では `main` へは未採用。採否判断用のメモとして残す。
- cache file は `target/surtr-stdlib-cache/std.semantic`。`SURTR_STDLIB_CACHE_DIR` 指定時はその directory 配下へ保存する。
- cache payload 候補は `DeclarationIndex` / `ResolveResumeState` / `ScarCheckpoint` / precompiled stdlib `Bytecode` / doc metadata / auto-import metadata。
- `spire::Ast` 全体の永続化は避け、cache hit 時も stdlib parse と doc collection は実行し、resolve / typecheck / codegen を cache から復元する方針を試した。
- cache key 候補は schema version、`xldr` crate version、compile policy version、`BUILTIN_METAS`、`BUILTIN_TYPE_METAS`、default stdlib source hash。
- cache miss / corrupt / schema mismatch / key mismatch / read-write failure は user-visible error にせず source rebuild へ fallback する。
- test build cost を抑えるため、serde derive は cache payload が実際に参照する型に限定する。`spire::Ast` 全体や `scar::TypedNode` 全体は永続化しない。
- branch では serialize / deserialize の stack 消費が大きい箇所に備え、大きめ stack の worker thread に逃がす案も合わせて試した。

計測:

```bash
rm -rf target/surtr-stdlib-cache
SURTR_SCAR_PROFILE=1 /usr/bin/time -p ./target/debug/surtr build tests/profile/heavy_compile.srt /tmp/surtr-heavy-cold.eldr
SURTR_SCAR_PROFILE=1 /usr/bin/time -p ./target/debug/surtr build tests/profile/heavy_compile.srt /tmp/surtr-heavy-hit.eldr
```

| item | cold miss | disk hit |
|---|---:|---:|
| `surtr build` real | 1.69s | 0.12s |
| `surtr build` user | 0.81s | 0.11s |
| stdlib Scar total | 801.045ms | skipped |
| post-stdlib Scar total | 31.501ms | 40.625ms |
| post-stdlib stmt count | 39 | 39 |

読み取り:

- cold miss では source から snapshot を再構築し、cache write も行うため v1 cold より重い。
- disk hit では default stdlib の parse / resolve / typecheck / codegen を disk snapshot から復元し、Scar は user/include 分だけを処理する。
- disk cache は compile-time artifact であり、壊れても通常 compile に戻る。

branch 実装で別途確認した事項:

- `.eldr` 形式は変更しない
- cache payload は compile-time artifact としてのみ扱う
- 壊れた cache を拾っても通常 compile へ戻ることを最優先にする
- 採用する場合は、serialize 対象型の境界をこれ以上広げずに済むかを先に見直す
