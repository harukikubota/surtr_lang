# `.srt` ソースのコンパイル時間短縮

調査基準: 2026-10-15、`a0d430a609da73f7f35bbba7a01f41f9b28dfc09`。
本書はコード調査に基づく候補と測定計画であり、最適化の実装仕様を確定するものではない。
今回の現行バイナリによる測定は [基準測定](04_baseline.md) に分ける。

追加依頼で、以下の候補に対応する複数の測定入力を整備した。[入力一覧](../../tests/profile/scar_compile/README.md) と [検証記録](05_compile_inputs.md) を参照。

**2026-10-16の実施:** 最初の一候補としてP1を実装した。全24入力・各条件10回のbefore / after比較と、通常debug stackの64段generic chain修正を含む検証結果は [06_deferror_scope.md](06_deferror_scope.md) を参照。以下のP1のコード説明は調査時点の挙動であり、現行は専用scopeへ置換済み。P2a / P2b以降の最適化は未実施である。

## 1. 最初に測るもの

主指標は、同じ `.srt` 入力をコンパイルしたときの対象フェーズと `compile_total` の時間とする。
cold の標準定義が対象なら `stdlib_load` とその内部の Scar 処理、user suffix が対象なら `typecheck` を見る。
Rust の `cargo build`、nextest の実行時間、生成済み `.eldr` の VM 実行時間は別の指標である。
ファイル分割だけで `.srt` のコンパイルが速くなるとは判断しない。

今回の baseline と別走行の profile から、cold では `deferror` の本体検査、disk hit では
trait/impl 追加時の predeclare と specialization を優先して調べる。
trait の identity 検索は、費用を局所化できてから選ぶ次候補とする。
以下の観測は現行実装の時間であり、最適化後の短縮実績ではない。

## 2. すでにある対策と観測手段

| 現行の対策 | コード上の確認先 | 今回の扱い |
|---|---|---|
| stdlib の semantic snapshot、process 内共有、disk cache、cold build の single flight | [Xldr](../../crates/xldr/src/lib.rs) の `build_stdlib_snapshot`、`default_stdlib_semantic_snapshot` | 新規導入案にしない。cold / disk hit / process hit を分けて測る |
| stdlib prefix から resolve / Scar / Forge を再開 | [Rune compile](../../crates/rune/src/compile.rs) の `compile_source_with_measurement` | stdlib を毎回全型検査している前提を置かない |
| one-shot suffix の in-place 型検査 | [Scar session](../../crates/scar/src/checker/session.rs) の `typecheck_staged_program_in_place_with_context` | transactional API と区別する。失敗後の session を再利用できる契約に変えない |
| trait / impl 追加のない suffix で predeclare の全体処理を省略 | [predeclare](../../crates/scar/src/checker/predeclare/mod.rs) の `declared_trait`、`declared_trait_impl`、`trait_impl_keys_in_stmts` | [既存計画](../optimize/014_scar_incremental_predeclare_fast_path.md) は実装と照合済み |
| 型・enum metadata の `Arc` 共有と enum の constructor/tag index | [TypeEnv](../../crates/scar/src/env.rs) | metadata の共有は既存。外側の map や推論状態の複製とは分ける |
| trait 定義・impl metadata と保持済み定義の共有、base trait ごとの impl index | [Checker state](../../crates/scar/src/checker/mod.rs) の `TraitDefinitions`、`TraitImplementations`、`SpecializableDefinitions`、`index_trait_impl` | 「毎回すべての impl 本体を深く複製する」という前提を置かない |
| relation probe の対象型に含まれる推論変数だけを保存し、concrete / rigid-only 比較では保存を省略 | [relations](../../crates/scar/src/checker/relations.rs) の `type_relation_checkpoint_for` | checkpoint 全体の clone 削減として再提案しない |
| specialization key による再利用、trait method 所属 index の一括構築 | [specialize](../../crates/scar/src/checker/specialize/mod.rs) の `specialize_program`、`ensure_specialized_def` | key の再利用は維持し、分類・走査費用を別に調べる |

[過去の計測手段監査](../optimize/011_compile_measurement_options_audit.md) の前半にある
「phase が未実装で `n/a`」という記述は調査当時の状態である。現行の
[観測仕様](../../docs/dev/Rune_observability.md) と
[measurement 実装](../../crates/rune/src/measurement.rs) には
`--phase-times-json`、各 phase の `executed` / `skipped`、マイクロ秒、cache 状態が存在する。
その追加を新規作業に含めない。

### 測定値の意味

- `build INPUT OUTPUT --phase-times-json` は compile-only の既存入口である。[build 実装](../../crates/rune/src/commands/build.rs) は final artifact cache を使わず、JSON の `cache.artifact` は `not_used` になる。`SURTR_RUN_CACHE=0` は `run` を測る場合の指定であり、`build` の条件を変えない。
- `typecheck` は prefix からの Scar session 復元と user/module suffix の型検査を囲む値である。stdlib cold build の Scar 時間は `stdlib_load` に含まれ、`typecheck` に合算されない。
- `compile_total` は plan、ソース収集、stdlib、各フェーズ、encode、output write などを含む。`parse` は `parse_modules` と `parse_user` の集約なので、すべての JSON 項目を足すと二重計上になる。
- disk hit でも標準ソースの収集、module AST の parse、docs/signature の収集を行ってから semantic payload を読む。`stdlib_load` が残ることは、Scar が標準定義を再型検査した証拠にはならない。
- `SURTR_SCAR_PROFILE=1` は `scar-profile` / `scar-phase` と statement/kind 集計を stderr に出す。イベントには入れ子があり、合計を総時間と比較しない。cold 時には標準定義と suffix の複数レポートが出るため、順序と入力条件を保持する。
- profiler は計時・集計の費用を加える。採用判定用の時間は profiler を無効にし、原因調査だけ別走行にする。現行の細目だけでは specialization 内の各走査や map 複製件数までは分からない。
- 現行 CLI は成功時の timing report を主な入口とする。拒否入力の診断検証に、必ず phase JSON が出ると仮定しない。

## 3. 短縮候補

### P1: cold の `deferror` 本体検査と子 Checker

[cold profile](evidence/profile_stdlib_prewarm_cold.log) の標準定義部分は、
Scar total 287.595ms、`DeferrorDef` 174件で163.654msだった。
同じレポートの child spawn は174回42.582ms、absorb は174回15.516msである。
これらは包含関係のある計時なので加算しない。profile 有効時の一走行であり、
163.654msすべてを除去できると解釈しない。

[definitions/nominal.rs](../../crates/scar/src/checker/definitions/nominal.rs) の `check_deferror_def` は、
各宣言で `self.env.clone()`、`spawn_child_checker`、本体の検査、`absorb_child_progress` を行う。
このコードと回数の一致は、cold の子 Checker 費用を調べる直接の根拠になる。
通常の関数本体には `check_body_in_isolated_scope` と `push_var_scope` / `pop_var_scope` が既存なので、
その状態保存・復元契約と `deferror` 固有の契約を比較する。
単純な String 本体だけを特別扱いして検査を飛ばす案にはしない。

最初の案は `deferror` の本体検査だけに閉じた scope 化、または必要な進行状態の移動である。
先に error definition UID、return type、parameter bindings、capability、warnings、
substitutions の保存・成功時反映・失敗時復元を表にして、子 Checker と等価な境界を定める。
詳細な clone 箇所と共通の注意は後述の P3 にまとめる。

**確認入力:** 標準定義 cold、Payload なし/ありの `deferror`、constructor 引数、
直接の終端内部構築、型の異なる message、constructor 内の失敗、その後の別宣言。
`deferror` の個数だけを増やす入力で拡大時の費用も調べる。

**契約:** ErrorKind と Error 値の区別、Payload の禁止型・重複 field 拒否、
終端内部構築の制約、定義間のローカル状態分離を維持する。
目標は cold の `stdlib_load` を含む compile の短縮であり、disk-hit の user phase と分けて報告する。

### P2a: specialization の走査と固定点計算

[generic Extractor profile](evidence/profile_generic_extractor_specialization_disk_hit.log) は
Scar total 8.436msのうち `specialize_program` が3.096msだった。
現在の入力でも調査対象になる割合を持つが、以下の各走査の費用はまだ分離できていない。

[specialize.rs](../../crates/scar/src/checker/specialize/mod.rs) の `specialize_program` は、
`self.specializable_defs` と当該 suffix の定義を合わせ、次の処理を行う。

1. `collect_direct_pattern_requirements`。
2. 各定義の bound type variables と pending trait call の分類。
3. `propagate_callable_specialization_requirements` で呼出し辺を収集。
4. 要求が変わらなくなるまで全呼出し辺を繰り返し走査。
5. typed node を書き換え、生成定義と未解決 executable type argument を検査。

したがって、小さな suffix でも保持済み generic 定義の数に依存する仕事がある。
さらに同じ定義を目的別に複数回走査している。これはコード上の事実であり、
今回の入力で支配的かどうかは profile と基準測定を使って判断する。

最初の変更候補は、一回の `specialize_program` の中で定義から得る不変の summary を共有し、
要求の伝播を逆辺を持つ worklist にすること。まず compile 呼び出しの内部に閉じ、
永続 cache を増やさない。全体の最終検査は、到達しない未解決参照の拒否などの契約を
確認する前に省略しない。

その後も prefix 再走査が支配的なら、変更されない定義の summary を session に保持する案を別段階で検討する。
その案には定義追加、function index 再調整、rollback、trait/impl 追加との整合が必要であり、
単なる memoize 追加として扱わない。checkpoint の wire layout 変更は最初の案に含めない。

**確認入力:** 長い generic 呼出し鎖、相互再帰、同じ generic を同一型・異なる型で呼ぶ入力、
direct Pattern、RTA forwarding、trait helper、do の failure specialization。
定義順を反転した同内容も使い、走査順依存の悪化を調べる。

**契約:** specialization key の決定性、再帰用 function index の予約、未解決型引数の拒否、
診断元の span、生成定義の重複なし、rollback 後の sibling session 分離を維持する。

### P2b: trait/impl を追加する suffix の predeclare

[MonadFail profile](evidence/profile_monad_fail_priority_disk_hit.log) は、
Scar total 20.627msのうち `predeclare_traits` が5.548msだった。
この suffix は trait impl を持つため、「trait/impl の追加なし」の既存 fast path では除去できない。
なお計時区間には `activate_nominal_declaration_bounds` も含まれる。

[predeclare.rs](../../crates/scar/src/checker/predeclare/mod.rs) の impl 登録・constructor mapping・
親 impl 被覆証明・method signature 構築を局所的に計時し、
新規 impl に対して必要な検査と既存 impl への再検査を分ける。
追加宣言のみの検査へ狭める場合も、追加によって既存候補との重複や親被覆が変わる境界を先に固定する。
現段階で、5.548msの原因を特定の全表走査一つと断定しない。

**契約:** trait/impl 宣言順への非依存、親 impl 被覆、constructor slot mapping、
method type list/where 制約、重複・曖昧性・循環の拒否を保つ。

### P3: 子 Checker と session の状態複製

[Checker](../../crates/scar/src/checker/mod.rs) の `spawn_child_checker` は
`persistent_state_with_env` に加えて substitutions、pending obligations、active capabilities などを clone する。
`absorb_child_progress` は子の substitutions / bounds などを親へ clone して戻す。
一部 metadata の値は `Arc` で共有されるが、外側の `HashMap` や推論状態のコピーは残る。

調査時の child 作成は `check_deferror_def` と `trait_dependency_for_requirement` の probe に残っていた。
P1実施後は後者のprobeを維持し、前者を専用scopeへ置き換えた。変数表の保存cloneは残している。
通常関数・match arm・closure は既に `push_var_scope` / `pop_var_scope` を使うため、
これらの child 廃止を新規案にはしない。既存 scope 内の保存・復元にもコピーがある場合は別に測る。
`child_checker_spawn` / `child_checker_absorb` が支配的な入力を先に選び、
残る二経路のうち、親へ戻す状態が明確な一経路だけを対象に所有権の移動または変更差分の返却を検討する。
一括して全 Checker を可変共有にする案は、分岐の推論・warning・capability 消費が漏れるため採らない。

session の transactional API にある clone は失敗時に state を保つための契約である。
既存 in-place API に置き換えるだけでは incremental 利用の短縮案にならない。

**契約:** 失敗した candidate/branch から substitutions・obligations が漏れないこと、
typevar/tag/function index の整合、warnings の重複なし、失敗後の checkpoint 復元を確認する。

### P4: canonical trait proof の identity 検索

[trait_selection.rs](../../crates/scar/src/checker/trait_selection.rs) の
`prove_canonical_constraints` と `prove_canonical_capability` は、trait UID に対応する
trait を `self.traits.iter().find(...)` で探す。
base trait ごとの impl index は既存だが、この UID→trait key の探索は別である。

追加 index が有利か、trait proof を行う一回の処理で lookup を共有すれば十分かを測る。
永続 index にする場合は、trait 登録・checkpoint 復元・rollback で正本と一致する必要がある。
小さな trait 集合では index の準備や保持費用が増える可能性もあるため、
trait 数と obligation 数を別々に増やして比較する。

capability proof 全体の結果 cache は初手にしない。
`Deferred` / `Unsatisfied` / `Satisfied`、rigid variable、active capability の証拠、
constructor witness、追加 impl、循環検出が結果に影響する。
未束縛時の失敗を保存して、束縛後の成功を隠す設計を避ける。

**契約:** parameterized obligation、generic/specific impl の選択、曖昧性・循環の拒否、
constructor carrier、capability 消費、source location を保つ。

### 保留する案

- 型の arena / interning、union-find 全面置換、AST 全面的再設計は、上記の測定で必要性が示されるまで保留する。公開 `Ty` や serialize 契約への影響が大きい。
- relation 結果や trait proof の無条件な global cache は導入しない。推論状態と診断の文脈をキーから落とす危険がある。
- stdlib snapshot の形式変更、precompiled stdlib 配布は別課題にする。今回の Scar 内部最適化の効果に含めない。
- Rust module 分割、nextest bucket、テスト用 prepared prefix は開発時間の改善として扱う。user `.srt` の短縮実績と合算しない。

## 4. 測定シナリオ

| シナリオ | 条件 | 見る値 | 注意 |
|---|---|---|---|
| C: stdlib cold | 新規 process、試行ごとに空の専用 stdlib cache | `stdlib_load`、`typecheck`、`compile_total` | OS の filesystem/page cache まで cold にした意味ではない |
| D: stdlib disk hit | 同じ binary/source、事前に生成した専用 cache、新規 process | 上記 + `cache.stdlib=disk_hit` | process hit とは呼ばない |
| P: process warm | 同じ process で stdlib snapshot を準備し、独立した同じ suffix を繰り返す | Scar 本体と session 復元を別計測 | 現行 `build` CLI を複数回起動してもこの条件にはならない |
| I: incremental | 同じ session に成功 chunk を追加し、失敗→復元→成功も測る | 各 chunk、checkpoint/rollback、保持済み定義数 | 前 chunk の状態で結果が変わるので P と混ぜない |

P/I の専用計測 harness は今回追加していない。実装ターンでは既存の `ScarSession` / `ScarCheckpoint` と
Sigil の resume state を使い、parse/resolve を含む外側と Scar 呼び出しだけの内側を分ける。
`Resolved` 入力の clone、session の生成/復元、TypedProgram metadata 生成を測定区間に含めたかを記録する。
同じ Resolved ID を成長中の session に無条件に再投入せず、P は同一 checkpoint に戻し、
I は正しい resolver resume state から新しい chunk を作る。
REPL 全体のテスト時間を、この専用計測の代用にしない。

### 既存入力と不足する入力

| 入力 | 現在調べられる性質 | 限界・追加方針 |
|---|---|---|
| [stdlib_prewarm.srt](../../tests/profile/stdlib_prewarm.srt) | 標準定義と小さな suffix の固定費 | 代表的な重い user source ではない |
| [heavy_compile.srt](../../tests/profile/heavy_compile.srt) | 過去の具体型中心の計測入力 | 初回調査では `Err(NoneError)` で失敗。追加依頼で `Err(NoneError())` に修正して build 成功を確認した。初回の速度集計には含めていない |
| [monad_fail_priority.srt](../../tests/fixtures/script/pass/stdmod/monad_fail_priority.srt) | trait impl、Error、MonadFail/Alternative の組合せ | 今回 cold / disk hit 各5回成功。規模依存の代表値には拡張入力が必要 |
| [generic_extractor_specialization.srt](../../tests/fixtures/script/pass/patterns/generic_extractor_specialization.srt) | generic Extractor、capture、Pattern、specialization | 今回 cold / disk hit 各5回成功。定義数・呼出し鎖の系統的な拡張は別途必要 |
| [generic_trait_forwarding.srt](../../tests/fixtures/script/pass/functions/generic_trait_forwarding.srt) | generic trait forwarding の意味論 | 小さい fixture。速度差の採用判定は反復だけでなく入力規模を増やした対照が必要 |
| [parameterized obligation](../../tests/fixtures/script/pass/typecheck/trait_impl_parameterized_obligation_exact.srt) | trait 引数を含む obligation の厳密性 | trait 数・obligation 数を独立に増やす計測入力を将来追加 |
| [monad_transformers.srt](../../tests/fixtures/script/pass/stdmod/monad_transformers.srt) | carrier と高階の組合せ | 言語機能全体の代表値とはしない |

追加依頼で、64/128/256 Error 定義、16/32/64 generic 呼出し鎖と逆順宣言、Trait impl、nominal dependency の系列を用意した。機能別と module 入力も含む全入口は [cases.json](../../tests/profile/scar_compile/cases.json) を使う。機能別の既存 fixture は意味論の参照先として維持する。
今後の入力追加も、少なくとも三つの規模を用意する。
深さだけ増やす入力と、平坦な定義数だけ増やす入力を分ける。
具体型のみの対照、同型 specialization の反復、異型 specialization、match/closure 数、
trait 候補数を一度に変えず、候補ごとに一軸ずつ調べる。
入力数・byte数・typed node 数・保持済み定義数を可能な範囲で保存し、
時間が減った理由を処理省略や入力の意味の変化で説明していないか確認する。

## 5. 再現コマンド

リポジトリルートで実行する。以下は今後の比較でも使う手順であり、
今回実行した範囲・値は [基準測定](04_baseline.md) を正とする。
`cargo build` は測定の準備であり、`.srt` の時間に含めない。

```sh
measure_dir=$(mktemp -d /tmp/surtr-scar-measure.XXXXXX)
export CARGO_TARGET_DIR="$measure_dir/target"
cargo build --release -p rune --bin surtr
surtr_bin="$CARGO_TARGET_DIR/release/surtr"
git rev-parse HEAD > "$measure_dir/head.txt"
git diff --binary > "$measure_dir/source.diff"
git status --short > "$measure_dir/status.txt"
rustc -Vv > "$measure_dir/rustc.txt"
shasum -a 256 "$surtr_bin" tests/profile/stdlib_prewarm.srt \
  tests/fixtures/script/pass/stdmod/monad_fail_priority.srt \
  tests/fixtures/script/pass/patterns/generic_extractor_specialization.srt > "$measure_dir/sha256.txt"
```

入力ごとに一つの cold 試行と一つの disk-hit 試行を組にし、独立した cache directory で繰り返す。
終了コードが 0 以外なら、その走行を短縮実績へ含めず診断を保存する。

```sh
set -eu
for input_path in tests/profile/stdlib_prewarm.srt \
  tests/fixtures/script/pass/stdmod/monad_fail_priority.srt \
  tests/fixtures/script/pass/patterns/generic_extractor_specialization.srt; do
  input_name=$(basename "$input_path" .srt)
  for run_id in 1 2 3 4 5; do
    case_dir="$measure_dir/$input_name-$run_id"
    mkdir -p "$case_dir/cache"
    env -u SURTR_SCAR_PROFILE SURTR_STDLIB_CACHE_DIR="$case_dir/cache" \
      "$surtr_bin" build "$input_path" "$case_dir/cold.eldr" \
      --phase-times-json > "$case_dir/cold.stdout" 2> "$case_dir/cold.stderr"
    env -u SURTR_SCAR_PROFILE SURTR_STDLIB_CACHE_DIR="$case_dir/cache" \
      "$surtr_bin" build "$input_path" "$case_dir/disk.eldr" \
      --phase-times-json > "$case_dir/disk.stdout" 2> "$case_dir/disk.stderr"
  done
done
```

上記は専用 shell で実行し、`set -eu` で失敗時に終了する。
全試行を継続して収集する harness にする場合は各 process の exit code を別途保存する。
stderr は warning を含む可能性がある。JSON の一行を取り出し、
`cache.stdlib` が期待する `cold` / `disk_hit`、`cache.artifact=not_used`、
`phases.typecheck.status=executed`、`phases.execute.status=skipped` であることを検証する。
実行時間が短くても状態が違う走行は同じ集合に入れない。

profile は通常測定と分け、準備済み disk cache を利用する。

```sh
SURTR_SCAR_PROFILE=1 SURTR_STDLIB_CACHE_DIR="$measure_dir/generic_extractor_specialization-1/cache" \
  "$surtr_bin" build tests/fixtures/script/pass/patterns/generic_extractor_specialization.srt "$measure_dir/profile.eldr" \
  --phase-times-json > "$measure_dir/profile.stdout" 2> "$measure_dir/profile.stderr"
```

## 6. 比較・採用条件

1. before/after で同じ入力 hash、compiler build profile、Rust toolchain、CPU、電源条件を使う。source commit/diff と binary hash を保存し、別 checkout の古い binary を測らない。`lib/` を読む実行 cwd も固定する。
2. binary/source ごとに target と semantic cache を分ける。現行 cache key は compiler build fingerprint と標準ソース等を含むが、測定の追跡を key の実装だけに依存しない。測定中に同じ target を別タスクで更新しない。schema/VM version を上げて cache を無効化しない。
3. cold と disk hit は別集計とし、各走行値・中央値・最小最大を保存する。初回の5回は基準把握用。実装採用の比較は各条件10回以上を目安とし、before/after を交互に行って機械の負荷変化を確認する。
4. 対象フェーズの中央値が同条件のばらつきでは説明しにくい程度に短縮し、処理量の削減根拠があることを確認する。P1 は cold の `stdlib_load` と `compile_total`、P2 は suffix の `typecheck` と `compile_total` を見る。P1 の Scar 内部への帰属は別の profile で確認する。代表入力に繰り返し現れる悪化があれば、対象入力の改善だけで採用しない。差が小さい場合は未確定とし、操作回数・入力規模を増やす。具体的な比率目標は、候補と入力を固定した実装計画で定める。
5. algorithm を変える場合は、入力規模を増やしたときの走査回数・固定点の更新回数も見る。wall time が減っても、同じ小入力だけの最適化で大入力の増え方が悪化していれば採用しない。
6. 成功値、拒否条件、診断 reason・span・hint、specialization identity、checkpoint 分離を固定する。速度のために曖昧な fallback、旧実装の切替 flag、未解決型の受入れを加えない。
7. 通常の debug stack でも深い specialization が成功することを確認する。[既存の stack 使用量調査](../optimize/018_scar_specialization_stack_usage.md) の再発を release 計測だけで見落とさない。
8. 回帰テストは対象契約から始める。局所変更は Scar、session/phase 境界を変える場合は Xldr/Rune の対象検証へ広げる。大規模リファクタリング全体の最終判定では CI profile の workspace と標準 Surtr テストを実行し、性能測定とは別に結果を報告する。

## 7. 次に進む順序

1. 現行 baseline の cache 状態と phase 内訳を確認する。
2. cold の P1、disk-hit の P2a/P2b を別の目的として選び、対象内部の費用を局所化する。P3 の一般化と P4 は費用の根拠が得られるまで保留する。
3. 選んだ一候補に対応する規模別入力と契約テストを先に定義する。
4. 最初の実装は一回の compile 呼び出し内に閉じた改善を優先する。
5. 採用条件を満たしてから、session をまたぐ summary 保持や状態表現の変更を別仕様にする。

P1は実装・検証・性能比較済み。規模別入力も含む全24入口を各条件10回測定した。P2a / P2b以降の最適化とprocess warm / incremental専用harnessは未実施である。
コードから確認できた反復処理を、まだ確認していない性能効果に読み替えない。
