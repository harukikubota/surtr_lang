# Extractor 返り値契約の更改案（独立ドラフト）

## 状態と目的

本書は、Extractor の返り値契約を現行の `Option<T>` から変更する必要があるか、その場合にどの結果状態を表現するかを決めるための独立した検討稿である。案の比較と未決事項を記録する。ここに書かれた候補は採用仕様ではなく、`Result` 返却も確定していない。

現行の実装計画には Extractor の返り値を更改する作業、変更後の契約、または採用候補の記載がない。したがって、他機能の実装順や完了状態から更改の方向を推定しない。本書は他の作業の前提を置かず、Extractor 自体の契約だけを決める。

## 現行契約

Extractor は `match`、`=?`、partial pattern における分解関数であり、通常式として呼び出す関数ではない。1個の入力値を受け取り、`Option<T>` を返す。`T` は1値、または複数のpattern bindingに対応するtupleである。

- `Option::Some(payload)` は分解成功を表す。payload は pattern の子要素へ渡される。
- `Option::None` は no-match を表す。match では次のarmへ進み、partial pattern の呼び出し側ではその文脈のfailure routeへ進む。
- Option の variant 以外の値、または未知のtagは正当なExtractor結果ではなく、runtime failureとなる。
- 入力型、戻りpayloadの型とarity、patternの子型は型検査時に照合する。
- Extractorの呼び出しはpattern loweringが所有し、通常の式評価や一般関数呼び出しとして公開しない。

現行実装はScarで戻り型がOptionか、payloadが単値またはtupleかを検査し、Forgeで成功tagのpayloadを分配する。no-match tagはpattern failure branchへ送り、認識できないtagは`InvalidMatchResult`とする。標準の`uncons`は内部的に同じmatch/no-match契約へ接続される。

## 変更目的と対象外

更改を行う場合の目的は、Extractorの成功とno-matchに加え、Extractor自身が報告する実行時失敗が必要かを明確にし、その区別を型と制御フローで表せるようにすることである。目的が必要性として確認されない限り、返り値変更を行わない。

この検討では、pattern構文、入力型の推論、pattern totality、網羅性規則、Extractorの通常関数化、型クラスによる動的選択、特定のcarrierや型名に依存するcompiler特例を決めない。Extractorをどの構文位置で使えるかは現行規則を維持する前提とし、更改案はその規則の変更理由を別途示さなければならない。

## 候補

### A. 現行Option契約を維持する

`Option<T>`を維持し、Extractor自身が報告する実行時失敗状態は追加しない。`Some`が成功、`None`がno-match、その他の結果は契約違反として扱う。

利点は既存の型規則・lowering・利用例を保てること、matchのno-matchを通常の失敗値と混ぜないこと。制約はExtractorが理由付きの実行時失敗をpattern evaluatorへ返せないこと。現行動作で要件を満たすなら、変更面積が最小である。

### B. `Result<T, E>` を二状態の結果として使う

`Ok(payload)`を成功、`Err(error)`をno-matchまたは失敗とする案。ただし`Err`がno-matchを意味するのか、利用者に報告する実行時失敗を意味するのか、両方を表すのかが未定義ではならない。

`Err`をno-matchと定義すれば、型付きの失敗理由を通常のpattern failureと区別できない。`Err`を実行時失敗と定義すれば、no-matchをどこで表すかが残る。現在の二状態Option契約を単純置換するだけでは三つ目の状態を表せず、意味論を定めずに採用できない。

### C. `Result<Option<T>, E>` を三状態の結果として使う

- `Ok(Some(payload))`: 分解成功。
- `Ok(None)`: no-match。
- `Err(error)`: Extractorが報告する実行時失敗。

この案は三つの状態を別々に表せる一方、現行ではOptionだけを許すExtractor返却契約を変更する根拠、失敗値をどのpattern文脈でどう観測するか、既存failure routeと二重化しないかを決める必要がある。ネストしたwrapperをExtractor専用の暗黙変換で平坦化してはならない。

### D. 三状態を専用の明示型で表す

Extractor専用の結果型を導入し、`Matched(payload)`、`NoMatch`、必要なら`Failed(error)`を明示する案。状態名と型が契約を直接表すが、新しい型構文または宣言、variant、型検査、標準定義、loweringを追加する必要がある。既存の型・式の合成で同じ契約を明快に表せない根拠を示してから検討する。

## 設計判断として未決の項目

1. 現行の二状態が不足する具体的な利用例と、それがExtractorの責務に属する理由は何か。
2. 追加状態は、no-matchとは異なる利用者観測可能な実行時失敗か。必要なpayload型、表示、伝播先は何か。
3. 実行時失敗を導入する場合、失敗時に後続match armを評価するのか、現在の制御フローを中断するのか。すべてのExtractor利用位置で同じ規則にできるか。
4. `match`、`=?`、partial patternで、no-matchと実行時失敗をそれぞれどう扱うか。既存のfailure value、error detail、source spanとの関係は何か。
5. 現行の一般関数では`Result<T, E>`は戻り値シグネチャ限定の補助表記であり、Eは`deferror`型、実行時の失敗値はabstract `Error`へ収束する。Extractor自体はOption返却限定である。この既存のResult型構成をExtractor返却にも認める必要があるか。採用時は既存Result/error規則を維持し、変更が必要ならその理由と範囲を別途明示する。
6. payloadのtuple/arity規則、型検査、入力制約、builtin extractorにも同じ規則を適用できるか。
7. `uncons`など、失敗理由が空入力/no-matchとして既に定義されるbuiltinはどの状態を返すか。
8. 未知variant/tagは依然として内部契約違反として即時failureとするか。利用者向け`Failed`値と混同しない仕組みは何か。
9. 旧Extractorの拒否例をどのように維持し、新契約で拒否すべき型・状態をどう区別するか。

## いずれの候補にも必要な不変条件

- Extractorの入力はちょうど1値である。
- 成功payloadの型とpattern arityは静的に一致する。arity検査後にだけ子patternを対応付ける。
- no-matchは明示された一つの経路を通り、成功payloadとして誤って分配されない。
- 契約にないvariant/tagや不正なruntime representationをfallbackでno-matchへ変換しない。
- Extractorはpattern側から必要な回数だけ呼ばれ、ひとつの判定で複数回評価しない。
- 診断は利用者のpattern位置と、必要な場合はExtractor定義位置を示す。診断の意味を自由形式message解析へ依存させない。
- 既存のpattern照合、SafeBind相当の利用位置、標準builtinに適用できる一貫した規則を定める。

## 受入条件（候補決定後に具体化する）

- 選んだ結果状態を、型・意味・制御フローで一意に定義し、各利用位置に適用する。
- 成功・no-match・もし採用するなら実行時失敗について、ユーザー定義Extractorとbuiltinを含む成功・拒否境界を固定する。
- payloadの単値・tuple、arity一致・不一致、入力型一致・不一致を検証する。
- Extractorを一度だけ呼び出すこと、成功payloadを正しく束縛すること、no-match時に後続bindingを導入しないことを検証する。
- 不正な戻り型、未知variant/tag、runtime表現不一致は明示的に拒否し、既存failureへ曖昧に丸めない。
- 変更対象のparser / typechecker / typed representation / lowering / runtime境界に対し、各責務を一つの正本で定義する。
- 現行契約を置き換える場合、旧型に対する拒否テストを残し、診断と利用者向け説明を新契約に合わせる。
- 既存の言語仕様と矛盾が見つかった場合、その矛盾を解消するまでは候補を採用仕様にしない。

候補選択、Errorの形、各利用位置での失敗処理、既存契約との移行範囲が確定するまでは、本書はdraftのままとする。

## 独立した作業と検証

本稿の採否・実装は独立した別タスクで管理し、SafeBind、do、MonadT、Generatorの着手・完了を前提条件にしない。
これらの機能も本稿の採用を待たず、現行のOption-returning Extractorで検証・完了できる。
後に既存consumerが増えていれば、その時点の契約との整合を確認するが、別機能の導入自体は要求しない。

返却型・失敗の評価規則を変える場合はlevel4とする。仕様決定、成功・拒否境界のテスト、最小実装、
旧経路の削除、正本更新、独立レビューの順に進める。候補Aを選ぶ場合は返却契約の実装変更は不要である。

実装時の検証は変更した責務から始める。Scarの返却型・payload/arity、Forgeの単一評価・分岐、
Rune script fixtureの成功/no-match/契約違反、Xldrの診断後継続を確認し、
`rtk cargo nextest run --profile ci --workspace`と`cargo run -- test --quiet --all`で全体を検証する。
移管先は`doc/要件定義v9.md`のExtractor・tuple・SafeBind節、`docs/site/extractors.md`とpattern関連文書、
`docs/dev/テスト方針.md`、変更した標準Extractorの`@doc`である。これらの所在は作業案内であり、本稿の契約は本文だけで読める。
