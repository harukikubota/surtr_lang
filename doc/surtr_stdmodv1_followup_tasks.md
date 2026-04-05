# Phase Standard Modules V1: 後続タスク整理

最終更新日: 2026-04-05

---

## 0. 位置づけ

StdModV1 の大枠実装が一通り入ったため、残っている「破壊的変更」「テスト拡充」「ドキュメント更新」「bootstrap 分離」「builtin 由来の error 定義整理」を、このファイルへ切り出して管理する。

- 参照元: [doc/surtr_stdmodv1_issue_reorg.md](./surtr_stdmodv1_issue_reorg.md)
- 本ファイルの役割: 後続で着手する具体タスクの整理
- 参照元の役割: StdModV1 再編の背景、実施済み Issue、設計判断の経緯

---

## 1. スコープ

このファイルで扱うのは次の 5 系統である。

1. 既存 API 利用箇所に対する破壊的変更の整理
2. テスト拡充
3. ドキュメント更新
4. `bootstrap.srt` の `Bootstrap` / `Kernel` 分離と `@@builtin` の利用境界整理
5. error 定義の配置ルール整理

今は扱わないもの:

- project runner DSL の詳細拡張
- REPL command set の拡張
- runtime error taxonomy の全面改訂
- 標準モジュール以外の大規模機能追加

---

## 1.1 確定事項

以下は未確定ではなく、後続タスクの前提として固定する。

### 標準モジュールのロード順

標準モジュールとユーザ拡張のロード順は次で固定する。

```text
Bootstrap -> Kernel -> [他標準モジュール] -> ユーザ拡張
```

- ユーザから呼ばれる Loader はこの読み込み順を保持する
- Loader API によって末尾へユーザ拡張を追加していく
- Surtr コード側のラッパーは `&mut Loader` を操作する形を基本とする
- ユーザ拡張は標準モジュールより前には入れない

### auto import ポリシー

- Kernelモジュールまでは auto import 対象とする
- Kernelモジュールに対する明示的な `import` は compile error とする
- 標準モジュール、ユーザ拡張は auto import しない前提で扱い、必要に応じて明示 import する
- したがって `Bootstrap` / `Kernel` はロード順で利用可能になるが、user code からの明示 import 対象としては扱わない

### 互換移行ポリシー

- 既存 API は互換維持せず全面廃止する
- 未リリース段階のため、移行用の互換レイヤーは原則残さない
- 古い呼び出し経路は段階維持ではなく、新 API への置換を前提に整理する

### 決定性ポリシー

- 将来の並列コンパイルを見据え、常に完全に同一の bytecode 列を保証対象にはしない
- 決定性は「bytecode 集合が同一であること」を基本とする
- たとえば `func_id` の差異のような、実行意味に影響しない並び差は許容しうる
- ただし 1 ファイルずつの step compile では、同一入力から同一 bytecode を出力できることを保証対象にする

### `Bootstrap` を先に置く意図

- 汎用 error は `bootstrap.srt` 側へ先に定義する
- これは `Kernel` より先にコンパイル・解決しておく意図を階層で表現するためである
- 将来の並列コンパイル時に、`Kernel` 側が汎用 error 定義待ちで不必要に停滞する時間を減らす
- マクロ起点の分離は現時点では考慮しない

### `Bootstrap` / `Kernel` の責務分担

- 組込み宣言は `Bootstrap` に置く
- 組込み以外の標準関数・標準定義は `Kernel` に置く
- 汎用 error 定義は当面 `bootstrap.srt` に置く
- したがって `Bootstrap` は「builtin 宣言 + 汎用 error 定義」を持つ先行解決層とする
- `Kernel` は「非 builtin の標準 API」を持つ層とする

### import 検査ポリシー

- import 検査はファイル単位で適用する
- ファイルトップの import は、そのファイル内の複数定義すべてに効く
- モジュール内 import は今後対応とし、現時点では考えない
- すでに import 済みのモジュールを再度指定したら compile error とする
- すでに import 済みのモジュール配下関数を、同一モジュール指定経由で再指定したら compile error とする
- 例:
  - `import Kernel` 後に `import Kernel::add` はエラー
  - `import Kernel::add` 後に `import Kernel` もエラー
  - `import Kernel::add` 後に `import Kernel::add` もエラー

この検査は import 解決時に行い、対象モジュールまたは対象関数がすでに同一ファイルで import 済みかどうかで判定する。

---

## 2. タスク一覧

### Task 1: 既存 API 利用箇所の破壊的変更整理

#### 目的

StdModV1 導入後の API 形状に合わせ、旧 API を使っている呼び出し側・テスト・補助コードを新経路へ統一する。

#### 対象

- Loader / script 実行 API の旧呼び出し経路
- `bootstrap.srt` 単一前提の参照
- entrypoint / `SourceRules` 導入前提の補助コード
- dump / debug / integration test が前提にしている旧メタデータ

#### 実施項目

- 旧 API を呼んでいる箇所を棚卸しし、移行対象を一覧化する
- 段階移行用に残っている互換レイヤーは削除対象として扱い、新 API へ全面移行する
- 破壊的変更が CLI 契約に波及する場合は、`integration` テストとヘルプ文言を同時更新する
- 参照名・ファイル名・モジュール名の変更が入る場合は、呼び出し元の修正単位をコミット分割可能な粒度へ落とす

#### 受け入れ条件

- 旧 API 前提のコードパスが残らない、または意図的に残すものが明記されている
- 破壊的変更が利用箇所ごとに追跡できる
- テストが旧 API への偶然依存で通っていない

---

### Task 2: テスト拡充

#### 目的

StdModV1 の設計変更が既存仕様を壊していないこと、そして新しい境界条件が固定されていることを保証する。

#### 実施項目

- 要件をテスト観点へ分解し、`spec / compile_errors / integration / unit` のどこで固定するかを決める
- 既存 API の破壊的変更に伴う回帰ケースを追加する
- `bootstrap.srt` 分離後のロード順、可視性、名前衝突、entrypoint 解決を固定する
- `@@builtin` が標準モジュールでは許可され、ユーザ拡張では禁止されることを `compile_errors` で固定する
- builtin 起点 error 定義が `Result` / `eprint` / 診断表示で従来どおり振る舞うことを `spec` / `integration` で固定する
- 標準モジュールが auto import され、明示 import が拒否されることを `compile_errors` / `integration` で固定する
- 同一ファイル内で同一標準モジュールまたは同一標準関数を再 import すると失敗することを `compile_errors` で固定する
- step compile では同一 bytecode、将来の並列前提では bytecode 集合同値を検証できる観点を用意する

#### 優先観点

- 標準モジュールの読み込み順が変わっても結果が安定すること
- `Bootstrap` と `Kernel` の分離後に import / qualified name が破綻しないこと
- 標準モジュールへの明示 import が禁止されること
- ファイル単位 import 検査で、同一モジュールおよび同一関数の二重 import が拒否されること
- 旧 bootstrap 単一前提の fixture が残っていないこと
- 標準モジュール外での `@@builtin` 利用が診断付きで拒否されること
- step compile では bytecode 出力が完全一致すること
- 並列化後も bytecode 集合としては同値であることを確認できること

#### 受け入れ条件

- 新規仕様に対して成功系・失敗系の最低限の fixture が揃う
- CLI 契約差分は `integration` で検出できる
- 追加した設計判断に対して、少なくとも 1 つ以上の再発防止テストがある

#### テスト方針

`Task 2` では、次の粒度でテストを追加する。

##### `compile_errors`

- 標準モジュールへの明示 import 禁止
  - `import Bootstrap`
  - `import Kernel`
  - `import Kernel::add`
- 同一ファイル内の重複 import 禁止
  - `import Kernel` 後に `import Kernel`
  - `import Kernel` 後に `import Kernel::add`
  - `import Kernel::add` 後に `import Kernel`
  - `import Kernel::add` 後に `import Kernel::add`
- ユーザ拡張での `@@builtin` 禁止
- `Bootstrap` / `Kernel` 分離後の名前衝突
  - 同一 module path 衝突
  - 先行ロードされる標準モジュールとの衝突

##### `spec`

- auto import 前提で標準 API が使えること
- `Bootstrap` 側に置いた汎用 error が `Result` / `match` / `eprint` で従来どおり使えること
- `Kernel` 側に置いた非 builtin 標準関数が import なしで使えること
- `bootstrap.srt` 分離後も既存の代表的成功ケースが壊れていないこと

##### `integration`

- Loader が `Bootstrap -> Kernel -> [他標準モジュール] -> ユーザ拡張` の順序を保持すること
- ファイルトップ import が、そのファイル内の複数 `defmod` に同時に効くこと
- 明示 import 禁止と重複 import 禁止が CLI 経由でも同じ診断になること
- `bootstrap.srt` 分離後の run / dump / repl 入口が壊れていないこと

##### `unit`

- import 解決時に file 単位の import 済み集合を判定できること
- module import 済み時に、その module 配下 function import を重複として検出できること
- `Bootstrap` と `Kernel` の source kind / module path / source descriptor が期待どおりになること
- step compile で同一入力から同一 bytecode が生成されること

##### 将来の並列コンパイル向け観点

- 現時点では完全な並列 compile テストは必須化しない
- ただし将来に備え、比較単位は「bytecode 列完全一致」ではなく「bytecode 集合同値」でも扱えるよう観点を残す
- 並列 compile 導入時に、`func_id` などの順序差を許容した比較方法を追加する

##### 追加順

1. `compile_errors` で禁止事項を固定する
2. `spec` で auto import と error 利用の成功系を固定する
3. `integration` で Loader 順序と CLI 契約を固定する
4. `unit` で import 判定と bytecode 決定性を固定する

---

### Task 3: ドキュメント更新

#### 目的

StdModV1 の現状を正本ドキュメントへ反映し、実装と文書のズレをなくす。

#### 更新対象

- `doc/要件定義v9.md`
- `doc/テスト方針.md`
- `doc/Xldr_spec.md`
- `doc/EldrVM_spec.md`
- `doc/open-issues.md`

#### 実施項目

- `CompileUnitKind` / `SourceKind` / `SourceRules` の確定仕様を正本へ反映する
- 標準モジュールの構成を `bootstrap.srt` 単体前提から更新する
- `@@builtin` の利用境界を明文化する
  - 標準モジュールでは利用可能
  - ユーザ拡張では利用不可
- error 定義の初期配置方針を明文化する
  - 共通 error は builtin 側で先に定義
  - 型固有 error は該当モジュール導入時に移設
- テスト方針に、標準モジュール境界と builtin 制約の回帰観点を追加する
- Xldr / Eldr の仕様書に、bootstrap 分離とロード契約が影響する箇所を反映する
- 未確定事項だけを `open-issues.md` に残し、確定事項は正本へ移す

#### 受け入れ条件

- 実装済みの仕様が正本に反映されている
- まだ未確定の項目だけが `open-issues.md` に残っている
- `bootstrap.srt` 単一前提の古い説明が残っていない

---

### Task 4: `bootstrap.srt` の `Bootstrap` / `Kernel` 分離

#### 目的

標準モジュールの責務を整理し、builtin 宣言と標準ライブラリ相当の定義を分ける。

#### 方針

- `bootstrap.srt` は `defmod Bootstrap`, `defmod Kernel` に分離する
- `@@builtin` は標準モジュール内では利用可能とする
- `@@builtin` はユーザ拡張では利用不可とする
- `Bootstrap` は汎用 error と builtin 宣言を先行解決する層として扱う
- `Kernel` は現時点の標準関数群を集約する層として扱う
- 標準モジュールは auto import 対象とし、明示 import は禁止する
- 組込み宣言は `Bootstrap`、それ以外の標準 API は `Kernel` に置く
- import 検査はファイル単位で行い、同一モジュールまたは同一関数の再 import を禁止する

#### 実施項目

- 現行 `lib/bootstrap.srt` の内容を builtin 宣言と通常定義へ分類する
- `Bootstrap` が持つ責務と `Kernel` が持つ責務を確定する
- Loader / bootstrap 読み込み / include 参照箇所を新構成へ追従させる
- Loader の公開 API が `Bootstrap -> Kernel -> [他標準モジュール] -> ユーザ拡張` の順序を必ず維持するよう整理する
- Surtr コード側ラッパーは `&mut Loader` を受け取り、ユーザ拡張を追加する形へ揃える
- 標準モジュールに対する明示 import をどの層で拒否するかを決め、診断を固定する
- import 解決時に、同一ファイル内の import 済みモジュール集合 / import 済み関数集合を見て重複を拒否する
- `import Kernel` と `import Kernel::add` のような「モジュール import 済み後の関数 import」も同一モジュール再指定として拒否する
- ファイルトップ import が複数 `defmod` へ同時に効くことを前提に、判定単位を module body ではなく file 単位へ固定する
- parser / validator / resolver / typechecker のどの層で `@@builtin` 利用境界を検証するか決め、実装を一本化する
- 標準モジュールのロード順と可視性の契約を明記する
- dump / diagnostic / source descriptor 上で、分離後の source 名と module path が追えるようにする

#### 追加ルール

- user code から `@@builtin` を書いて builtin を増やすことはできない
- builtin 追加・変更の正本は引き続き `eldr/src/builtin_registry.rs` の `BUILTINS` テーブルとする
- 標準モジュール側の `@@builtin` 宣言は、そのテーブルを参照する宣言層としてのみ扱う

#### 受け入れ条件

- builtin 宣言と標準モジュール定義の責務が分離されている
- `Bootstrap` / `Kernel` のどちらに何を置くかが明文化されている
- `Bootstrap` の先行コンパイル理由が error 解決順と並列待機削減の両面で説明できる
- 標準モジュールは auto import だけで利用でき、明示 import は compile error になる
- 同一ファイルでの重複 import が、モジュール単位・関数単位の両方で compile error になる
- ユーザ拡張で `@@builtin` を使うと compile error になる
- 標準モジュール読み込み後も既存プログラムの挙動が維持される、または差分が明記される

---

### Task 5: error 定義の builtin 起点整理

#### 目的

error 定義の配置方針を先に固定し、後から型別モジュールへ安全に移せるようにする。

#### 方針

- 共通 error 定義は最初に builtin 側へ置く
- 型固有の error 定義は、その型を扱う標準モジュールを作る段階で移行する
- 共通 error を `Bootstrap` 側へ置くことで、`Kernel` より先に解決させる意図を構造で表す

#### 実施項目

- 現在 builtin 側に置くべき error と、将来モジュールへ移すべき error を分類する
- 移行後も user-visible な名前、tag、診断表示、`eprint` 出力が破綻しない条件を整理する
- `TypeRegistry` / error template / builtin 初期化順への影響を確認する
- 将来移行する error については、移設条件と移設先候補をメモ化する

#### 受け入れ条件

- error 定義の初期配置ルールが一意に決まっている
- 将来の移行タイミングが曖昧なままになっていない
- tag / 表示 / 既存 fixture への影響点が洗い出されている

---

## 3. 追加で必要な作業

上の 5 タスクに加えて、次は先に押さえておいた方がよい。

### Additional 1: 標準モジュールのロード契約固定

- `Bootstrap -> Kernel -> [他標準モジュール] -> ユーザ拡張` の読み込み順をコードと文書で固定する
- `&mut Loader` を操作するラッパー API で、ユーザ拡張が後段に積まれることを保証する
- 標準モジュールは auto import 対象、明示 import は禁止とする
- 相互参照の可否
- script / project / REPL で同一契約にするかどうか

この契約が曖昧だと、実装は動いてもテストとドキュメントがぶれやすい。

### Additional 2: 互換移行の診断ポリシー

- 既存 API は未リリースのため全面廃止とし、互換レイヤーは残さない
- 移行診断をどこまで出すかは実装都合で決めてよいが、旧 API 温存はしない

### Additional 3: 決定性の回帰確認

- step compile での完全一致 bytecode
- 並列化を見据えた bytecode 集合同値
- `unique_id`
- `TypeRegistry` tag
- dump に出る module / source / entry メタデータ

bootstrap 分離と error 移設は決定性を壊しやすいので、単体の実装修正より先に観点を固定しておく。

---

## 4. 推奨実施順

1. `Task 4` の責務分解と利用境界を先に固定する
2. `Task 5` の error 配置方針を確定する
3. `Task 1` で既存 API 利用箇所の破壊的変更を整理する
4. `Task 2` で回帰テストを追加する
5. `Task 3` で正本ドキュメントへ反映する

理由:

- `bootstrap.srt` 分離と `@@builtin` 制約が、API・テスト・ドキュメント全部に波及するため
- error 配置方針が先に決まっていないと、標準モジュール分離後の責務が再度ぶれるため

---

## 5. コミット粒度の目安

- `Commit-A`: 標準モジュール分離方針の固定と最小実装
- `Commit-B`: `@@builtin` 利用境界の検証追加
- `Commit-C`: error 定義の builtin 起点整理
- `Commit-D`: 破壊的変更に伴う呼び出し側更新
- `Commit-E`: fixture / integration / unit の拡充
- `Commit-F`: 正本ドキュメント更新

原則:

- 基盤変更と fixture 更新は可能な限り分ける
- ドキュメントだけで完結する確定事項は、コード変更前に先行反映してよい

---

## 6. 完了条件

以下を満たしたら、この追補タスク群は一段落とする。

- 標準モジュール構成が `Bootstrap` / `Kernel` ベースで安定している
- `@@builtin` の利用境界が実装・テスト・文書で一致している
- error 定義の初期配置ルールが固定されている
- 破壊的変更の影響範囲が洗い出され、必要な移行が完了している
- 正本ドキュメントと実装のズレが解消されている
