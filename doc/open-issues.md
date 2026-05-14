# Surtr Open Issues

> 目的: V9 正本でまだ固定していない未解決事項だけを追跡する。
> 本ファイルは「未解決事項の台帳」であり、確定事項は `doc/要件定義v9.md`、開発者向け spec は `docs/dev/` 配下を正本とする。`doc/` は draft / input / tmp 置き場として扱う。

最終更新日: 2026-05-13

---

## Open Issues
### OI-005 マクロ展開段階と通常解決段階の分離

- 背景:
  - 現行 baseline では macro 段階は実質 no-op 前提だが、宣言収集と通常解決の間に macro slot を置ける構成になっている。
  - 将来 macro を入れると、宣言集合・ID 割り当て・依存解決順に直接影響する。
- 未確定点:
  - macro 展開を declaration index 前後のどこに置くか
  - 展開生成物の `unique_id` / `tag` の決定規則をどうするか
- 受け入れ条件:
  - macro あり / なしで同値プログラムの解決結果が一貫する。
  - macro 段階と通常 resolve 段階の責務境界が文書化される。
- テスト方針:
  - macro 導入時に `unit/spire` / `unit/sigil` で段階境界テストを追加する。
  - 展開後 IR の決定性比較を回帰基準にする。

### OI-010 `RichError` 表示形式の仕様化

- 背景:
  - `RichError::to_display_string` は現在 message を quoted form で表示している。
  - 実装上は安定しているが、user-facing display としてその形式を正本化するかは未確定のままである。
- 未確定点:
  - quoted form を正式仕様にするか、より自然な表示へ変えるか
  - 仕様化する場合に `docs/dev/EldrVM_spec.md` と rustdoc のどちらを正本にするか
- 受け入れ条件:
  - 採用した表示形式が docs とテストで一致する。
  - `Value::Error` の表示契約が将来ぶれない。
- テスト方針:
  - `unit/sindr` の `display_for_rich_error_*` 系テストで採用形式を固定する。

### OI-012 `.eldr` viewer follow-up

- 背景:
  - `.eldr` の viewer 向け chunk 基盤は入っているが、より深い debug 表示に必要な metadata はまだ最小限に留まっている。
  - `viewer.rs` 側にも source lookup や table 化の基盤はあるが、source compare や import 粒度の深掘りは未実装である。
- 未確定点:
  - `Dbgi` / `LocT` のような追加 table を入れるか
  - span に source id を持たせるか
  - import / literal metadata をどこまで viewer 向けに増やすか
- 受け入れ条件:
  - viewer が `.eldr` 単体で必要な debug 文脈を読める。
  - 追加 chunk 導入後も現行 decode 契約との後方整合が保たれる。
- テスト方針:
  - `unit/sindr` で chunk 整合性と参照先妥当性を固定する。
  - `integration` で dump 出力が必要テーブルを欠かさないことを維持する。

### OI-013 Tail Call Optimization の観測導線

- 背景:
  - TCO v1 は実装済みで、`tail_calls_optimized` も観測値として追加されている。
  - ただし、その観測値を CLI / dump / viewer にどう露出するかはまだ統一されていない。
- 未確定点:
  - `surtr run` / `surtr dump` / viewer で何を見せるか
  - tail-position call を bytecode 上で明示するか
  - span 診断や call trace を保ったまま追加最適化する方針
- 受け入れ条件:
  - 利用者が「TCO が効いたかどうか」を観測できる。
  - docs 間で TCO の適用範囲説明がぶれない。
- テスト方針:
  - tail recursion / mutual recursion / non-tail recursion の観測ケースを回帰基準にする。
  - CLI / JSON 露出を増やす場合は integration で形状を固定する。

### OI-015 test DSL I/O capture の `it` 単位分離

- 背景:
  - `Test::capture_stdout` / `Test::capture_stderr` は導入済みだが、現行は per-VM バッファ + drain cursor 方式である。
  - そのため同一 script VM 内の隣接 `it` に未読出力が流入する可能性が残っている。
- 未確定点:
  - `it` 境界で cursor reset を自動化するか
  - `test` / `describe` / `it` のどの粒度で capture scope を切るか
  - drain API のまま維持するか、peek 系 API を追加するか
- 受け入れ条件:
  - `it` 単位で deterministic に capture できる。
  - 後方互換方針が明確である。
- テスト方針:
  - `tests/integration/test_command.rs` に `it` 間混入防止ケースを追加する。
  - `lib/tests/*.srt` に capture API の推奨パターン fixture を用意する。

### OI-016 HashMap v1 follow-up

- 背景:
  - `HashMap<$V>`、`defmod HashMap`、表示契約は既に baseline として定着している。
  - 一方で literal sugar、追加 surface、runtime 内部表現の最適化余地はまだ未確定である。
- 未確定点:
  - `hash![...]` literal sugar を導入するか
  - `entries(map)` などの surface を v1.x で増やすか
  - runtime 内部表現を `Vec<(String, Value)>` のまま維持するか、補助 index を併設するか
- 受け入れ条件:
  - 採用方針が `doc/要件定義v9.md` / `docs/dev/EldrVM_spec.md` / `docs/dev/テスト方針.md` の3点で矛盾しない。
  - surface を増やす場合は std module と builtin metadata の整合が取れる。
- テスト方針:
  - literal 導入時は `unit/spire` / `unit/forge` / `spec` を同時に固定する。
  - runtime 表現変更時は insertion-order と display 契約を回帰基準にする。

### OI-017 Worker API 最終形

- 背景:
  - process runtime の大枠は `docs/dev/ProcessRuntime_spec.md` に固定したが、worker の user-facing API はまだ足場段階である。
  - `@call` / `@cast` は `CallResult` / `CastResult` 契約へ移行し、user-facing stop surface は `Stop(...)` に寄せた。
  - `Process::sleep` は scheduler timer、generic `Process::exit` は hidden のままにする方針も baseline として整理された。
- 未確定点:
  - `Worker::spawn` と `DynamicSupervisor::spawn` の最終 surface をどう並べるか
  - `adopt` / `handoff` を user-facing API として公開するか、runtime intrinsic に留めるか
  - `join` / `await` / `on_down` を generic `receive` の代替となる目的別 API としてどう分けるか
- 受け入れ条件:
  - worker 生成、所有権移譲、終了観測の surface が REPL / script / project で一貫する。
  - current process ownership を default とする方針と `DynamicSupervisor` 配下運用の両方を矛盾なく説明できる。
- テスト方針:
  - `tests/fixtures/script/pass/process_runtime/**` に worker spawn / await / on_down / supervisor 経由 spawn の成立ケースを追加する。
  - `tests/fixtures/script/fail/process_runtime/**` に不正な ownership / lifecycle API 組み合わせを追加する。

### OI-018 Process Runtime の最終同期項目

- 背景:
  - process runtime の基本契約は固まったが、ツーリング表示、boundary layer、VM 可視化、標準ライブラリ再編は最終段階で同期する想定のままである。
  - `Task::async -> TaskHandle`, `Task::await`, `Workers::submit/broadcast @timeout(...)`, waiting/deadline の baseline は docs と実装の主要経路で同期が進んだ。
  - worker stop の compile-time restriction と停止後 cleanup の主要経路は、現行の runtime / compile error / spec fixture で baseline 化が進んだ。
  - 現行実装でも `:doc`, `:sig`, `:type`, `:info`, `:facet`、runtime stats、標準 `Process` / `Task` API は存在するが、process-aware な見せ方は未完成である。
- 未確定点:
  - `ReplyLater` の layered timeout を正本 spec と runtime tests でどこまで明示固定するか
  - REPL / tooling 表示で init route、process API、singleton slot、supervisor tree をどこまで露出するか
  - domain error / runtime error / boot error を host outcome としてどう正規化するか
  - `RuntimeProcessSpec`, singleton slot, process table, deadline queue, waiting table, hidden message dispatch, supervisor tree をどの debug surface で見せるか
  - `Process`, `Task`, `File`, `Env`, `StdIn`, `StdOut`, `StdErr`, `Logger`, `DynamicSupervisor` の標準ライブラリ再編境界をどう切るか
- 受け入れ条件:
  - `ReplyLater` を維持するなら、outer timeout と callback 側 timeout の責務境界を正本 docs と tests の両方で説明できる。
  - 開発者向け spec と REPL / CLI / dump / viewer の観測導線が矛盾しない。
  - process runtime の host 境界と標準ライブラリ境界が `docs/dev/` と実装の両方で説明できる。
- テスト方針:
  - `ReplyLater` の timeout 契約を固定する場合は process runtime の spec / runtime test を追加して、outer timeout と callback 側 deadline の優先順位を回帰基準にする。
  - `unit/xldr` / `integration/repl` / `rune` integration で process-aware な表示と失敗形状を固定する。
  - `integration/build_roundtrip` / `run_eldr` / viewer 系テストで runtime metadata の可視化形状を固定する。

### OI-023 Task.Supervisor / Task-DynamicSupervisor link / worker lazy init

- 背景:
  - process runtime v2 では `Task`、`DynamicSupervisor`、worker lifecycle の土台は入ったが、Task supervision と worker lazy init は非対象として残っている。
  - `Task::async -> TaskHandle<T>`、`Task::await(task) -> Result<T>`、`Task::await @timeout(...)` は baseline として確定し、待機系 surface の最小契約は既存 spec と実装で同期した。
- 未確定点:
  - `Task.Supervisor` を独立 surface として持つか、`DynamicSupervisor` に統合するか
  - `Task` を supervisor 配下に link したときの ownership / restart / cancellation をどう扱うか
  - worker の lazy init や async init を `ProcessInit<T>` と別契約で導入するか
- 受け入れ条件:
  - Task と worker の lifecycle 契約が既存の singleton / worker / supervisor モデルと衝突しない。
  - supervisor / ownership / cancellation 契約を追加しても、既存の `TaskHandle` / `Task::await` baseline と衝突しない。
- テスト方針:
  - 仕様確定後に Task spawn/link/cancel、worker async init、supervisor 配下 completion の spec fixture を追加する。
  - compile-time 制約を入れる場合は process runtime 系 compile error fixture を増やす。

### OI-024 `File` v2 拡張境界

- 背景:
  - `File` v1 では `lib/file.srt` の `defmod File` として、UTF-8 text-only の `read` / `write` / `append` / `exists` / `delete` / `with_open` / `read_chunk` / `write_chunk` / `flush` を確定した。
  - resource lifetime は VM-owned open file table で管理し、`with_open` callback 終了時、VM run 終了時、interactive rollback 時の close を baseline として固定した。
  - 一方で、binary I/O、directory 操作、metadata、rename/copy、path surface、seek などは v1 から意図的に外している。
- 未確定点:
  - `Bytes` もしくは同等の binary surface を導入したうえで binary file API を追加するか
  - `mkdir`, `read_dir`, `rename`, `copy`, `metadata`, `canonicalize` のような host file-system helper を `File` に含めるか、別 module に分離するか
  - seek / cursor reposition を user-visible API として許可するか、それとも append/read sequential contract を維持するか
  - path を単なる `String` のまま扱うか、将来 `Path` 的な dedicated surface を持つか
  - file permission / mtime / size / kind を user-facing metadata としてどこまで露出するか
- 受け入れ条件:
  - v1 の cleanup guarantee と opaque `FileHandle` 契約を壊さずに拡張できる。
  - `FileOutHandler` の append-only runtime sink と、一般 file-system access の責務境界が docs と実装で混ざらない。
  - binary / directory / metadata を追加する場合も、`doc/要件定義v9.md`、`docs/dev/EldrVM_spec.md`、`lib/file.srt` の三者で同じ境界を説明できる。
- テスト方針:
  - binary surface を導入する場合は `unit/sindr` / `unit/eldr` で runtime value と builtin contract を固定し、`lib/tests/*.srt` では `./tmp/sandbox/` 配下だけを使う。
  - directory / metadata surface を増やす場合は Rust integration で実ファイル状態を検証しつつ、spec/compile error のどこに置くかを `docs/dev/テスト方針.md` と同期する。
  - seek や cursor API を導入する場合は rollback / shutdown cleanup と両立することを `eldr` unit test で固定する。

### OI-025 trait helper shadowing inside `Decode` impl

- 背景:
  - `Encode` / `Decode` helper は `@autoimport` trait helper として導入され、通常の unqualified call と pipeline partial call では既に機能している。
  - 一方で `impl Decode<JsonFormat, T> for JsonValue` の本文内で、さらに unqualified `decode(...)` を使って別 target へ再帰的に decode するケースでは、helper 名解決と current impl owner の shadowing 境界がまだ曖昧である。
  - 現行 baseline では `Json::as_string` などの helper を使うことで回避できるため、機能追加自体は完了扱いにしている。
- 未確定点:
  - impl 本文内の unqualified `decode(...)` を常に trait helper として優先解決するか
  - current impl owner method と trait helper capture のどちらを優先するか
  - call form、pipeline partial call、closure capture で同じ規則を共有できるか
- 受け入れ条件:
  - `impl Decode<..., T> for JsonValue` 本文内で unqualified `decode(...)` を使っても、意図した target type へ決定的に dispatch できる。
  - trait helper 名解決規則が通常 scope と impl scope の両方で一貫する。
  - 既存の `encode` / `decode` helper と method shadowing の挙動を壊さない。
- テスト方針:
  - `unit/sigil` / `unit/scar` に impl 本文内の nested `decode(...)` と pipeline partial call の回帰ケースを追加する。
  - `spec/json` に custom decoder が unqualified nested `decode(...)` を使う fixture を追加し、`Json::as_*` 回避なしで通ることを固定する。

### OI-026 FS / Shell surface naming and generic import ergonomics

- 背景:
  - FS / Shell v1 では `FileSystemPermissions` の permission flag を当初 `readonly` としていたが、`readonly` は field modifier として予約されているため field 名には使えない。
  - 実装では `FileSystemPermissions.read_only` として surface を固定した。
  - 同名 helper を持つ module を同じ file で unqualified import すると import conflict になる。`File.exists` と `FS.exists` はその具体例で、qualified call 自体は問題なく使える。
- 未確定点:
  - 予約語と同名の field を将来 escape syntax で許可するか、標準 surface では今後も別名を採用するか
  - `import FS::{path, join}` のような選択 import を推奨導線にするか
  - 同名 helper を持つ標準 module 同士を同時 import した場合の ergonomics を、alias import などで改善するか
- 受け入れ条件:
  - `FileSystemPermissions` の field 名が docs / stdlib / runtime display / tests で一致する。
  - `File` と `FS` の責務境界を崩さず、qualified call で常に曖昧性なく使える。
  - import ergonomics を改善する場合も、既存の import collision diagnostics を弱めない。
- テスト方針:
  - `lib/tests/file_system.srt` では `FS::*` を qualified call で使う形を維持する。
  - escape syntax や alias import を導入する場合は `compile_errors/modules` と `spec/modules` の両方に fixture を追加する。

### OI-027 Facet compose path embedding and pin-like path capture

- 背景:
  - `Facet::bulk_update(source) { ... }` は relative path をその場で列挙できるようになったが、entry 内に既存 path 値や composed path を埋め込む surface はまだ持っていない。
  - 現行の `Facet::chain` / `/` は通常式としては使える一方、bulk entry の left-hand path へ直接埋め込む記法は未定義である。
  - 将来的に Elixir の pin 演算子に近い surface を導入すれば、`^user_country.name` のような path capture / path embedding が候補になりうる。
- 未確定点:
  - bulk entry の左辺で既存 path 値を埋め込めるようにするか
  - その場合に `^path.segment` のような pin-like syntax を採用するか、別記法にするか
  - embedded path と通常 field / tuple / prism path の優先順位と parse 規則
  - lexical value capture と Facet path capture をどう区別するか
- 受け入れ条件:
  - 導入する場合、`Facet::bulk_update` の「通常コードに lower できる DSL」という説明を壊さない。
  - composed path / embedded path / dotted path / nested block が同一の path 意味論へ正規化される。
  - pin-like syntax を入れる場合、既存の capture / pattern / unary operator surface と衝突しない。
- テスト方針:
  - parser で embedded path / dotted extension / nested block の優先順位を固定する。
  - resolver / integration で embedded path が `Facet::set` / `Facet::over` の既存意味論へ正しく lower されることを回帰基準にする。
- syntax を導入しない場合も、`Facet::chain` を bulk 内で許可しない compile error fixture を残して境界を固定する。

### OI-028 generic `Range` comparison impl runtime mismatch

- 背景:
  - `Range<$A>` 自体は標準ライブラリへ追加済みで、`Range::normalized` と `Range::deconstruct` も generic に動作している。
  - 一方で `impl Compare for Range<impl Compare>`、`impl Eq for Range<impl Eq>`、`impl Neq for Range<impl Neq>` を実際の stdlib surface として有効化すると、runtime で `Call arity mismatch` に当たり安定しない。
  - parser / resolver / typechecker の構文面では既存 stdlib パターンに沿って受理されるため、問題は generic impl specialization か runtime call shape の同期にある可能性が高い。
- 未確定点:
  - `Range<impl Trait>` target の trait impl を runtime まで正しく specialize する責務がどこにあるか
  - tuple の `impl Compare for (impl Compare, ...)` と `Range<impl Compare>` で何が違うか
  - generic target impl と monomorphic impl の共存を許可するか、禁止するか
- 受け入れ条件:
  - `impl Compare for Range<impl Compare>`、`impl Eq for Range<impl Eq>`、`impl Neq for Range<impl Neq>` が runtime mismatch なしに動作する。
  - もしくは、generic impl target に現行制約があるなら docs / diagnostics / tests で明示される。
- テスト方針:
  - `lib/types/range.srt` を generic impl へ切り替える回帰ケースを `lib/tests/range.srt` に追加する。
  - `unit/scar` / `unit/forge` / `unit/eldr` で impl specialization 後の arity と call target shape を固定する。
  - `integration/repl` で `:sig Range`, `:doc Range(Int, Int)`, `Range("a", "c") == Range("a", "c")` の surface を最終形で固定する。

---

## Deferred Topics

以下は現行 baseline に含めず、必要時に reopen する将来課題。

- Project runner
  - source 操作 API、runner DSL、init command を別途仕様化する。
  - compile unit / source rules との責務分離を崩さないことを前提にする。
- REPL command 拡張
  - `:doc` は実装済み前提とし、その先の `:browse`, file ingest UX、補完改善を整理対象にする。
- closure の `expected=None` 推論強化
  - 期待型なしクロージャをどこまで多相的に扱うかを、let-generalization なしの baseline と矛盾しない範囲で再検討する。
  - 退避ケース:
    - `id = {|value| value}` が最初の呼び出しで単相化され、後続の別型呼び出しで `Argument type mismatch` になるケース
- runtime fuel budget surface
  - VM 内部の step / reduction budget と process quantum は baseline 化済み。
  - CLI / REPL / library execution から fuel budget を user-visible に設定する surface は未確定。
  - 退避ケース:
    - 再帰関数 `loop()` を CLI / REPL 設定の budget 超過として安定停止させる契約
  - 現状の正本テストは `lib/tests/*.srt` 側へ分離している。
- FuncLiteral surface の将来拡張
  - backtick capture / qualified path / operator capture は実装済み前提とし、それを超える surface 追加だけを reopen 対象にする。
- OOM / host failure policy
  - allocation failure を `RuntimeError`、process failure、host abort のどれとして扱うかと、利用者向け報告文言を将来固定する。
- Enum conversion helper
  - `defenum` 本体や `.idx` 廃止は確定済み前提とし、`Enum::from(Int)` / `Enum::try_from(Int)` の自動生成だけを未実装課題として扱う。

---

## 更新ルール

- 解決済み事項は本ファイルに残さず削除する。必要な履歴は正本仕様・関連 spec・コミット履歴で追跡する。
- 新規 Issue を追加するときは、少なくとも `背景`、`未確定点`、`受け入れ条件`、`テスト方針` を埋める。
- 実装先行で仕様が変わる場合は、先に本ファイルと正本仕様の整合を確認してからコード変更する。
- 将来課題のうち、まだ open issue として具体化していないものは `Deferred Topics` に置き、実装着手時に個別 OI へ昇格させる。
