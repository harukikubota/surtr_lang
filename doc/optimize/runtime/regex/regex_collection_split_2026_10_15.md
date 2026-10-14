# Regexの探索・収集を継続へ分ける

入力は[残り6分野の実施計画](../remaining_implementation_2026_10_14.md)。基準は `412c2514`。既存builtin IDと公開APIを保つlevel3変更。

find_all / split / replace_allの同期iterator loopを除去し、最大一回の全文位置指定探索を一reductionで進める。regex 1.12.3 / regex-automata 0.4.14の空一致抑制規則を維持する。重なる空一致を抑制した遷移では開始位置を一byte進めて再開し、UTF-8境界の選択はエンジンへ任せる。入力をsliceしてanchorを変えない。find_allの全文共有と一致順、splitの前後空文字、replace_allのcapture展開を維持する。

出力Listは共通の有界Builder完成を使う。置換出力とListの保存は独立した途中状態を持つ。取消・RuntimeErrorは部分結果を成功配送しない。純粋な遷移を残予算内でまとめ、全遷移を課金し、結果配送は共通stepに残す。compile・一探索・Value操作・String appendは同期であり、VM全体の実時間上限は保証しない。

最初に旧実装の同期Completeが受入テストで失敗することを確認する。空・非空・Unicode・anchor・空一致・named/optional captureの結果をregex crateと直接比較する。予算0/1/256、途中保存・取消、通常Task入口の切替えを検証し、最終workspace CI・標準全件と手組みVM/通常入口の測定を残す。

## 実装と検証結果

旧同期Completeで `collection must yield before searching` のRedを確認した。継続実装後、16 pattern × 85入力でfind_allの全span・全文・UTF-8境界、splitの全出力、5種類の置換参照をregex 1.12.3と直接比較して一致した。予算0/1/256、途中保存の独立再開と取消、既存の全文共有・無一致・captureアクセスの境界も成功。通常Task fixtureで3処理ともstartがcompletionより先に出ることを固定した。

最終Eldr 401件、workspace CI 2,584件、標準1,124件すべて成功。失敗・除外・未実行0。[検証記録](regex_collection_split_2026_10_15.validation.json)。fixture作成時の未定義APIは既存String::repeatへ修正し、意図したRedには含めていない。

一回の探索は同期のまま。重なる空一致を捨てた遷移にも一reductionを課金する。capture参照を含まない置換ではcapture取得を行わず、literalを追加する。旧3関数のiterator loopを削除し、新しいbuiltin ID・Opcode・Value fieldやVM/schema versionは加えていない。

## 費用測定

[手組みVMの原本](regex_collection_split_2026_10_15.jsonl)・[集計](regex_collection_split_2026_10_15.summary.json)・[全内容確認](regex_collection_split_2026_10_15.verify.jsonl)に、exact source / Cargo artifact / binary / runner hashを保存した。旧基準412c2514へ同じprobeを使い、独立count/time targetで保存したbinaryを前後6 sample、AB / BA同数で比較した。全内容確認40 processと測定240 process、1,440区間。buildやtestを測定へ重ねていない。

4,096-byte ASCIIのdense find_allで、runの要求量は1,691,295→1,126,847 bytes、counter無効時間中央値は0.538→0.475 ms、範囲は0.522–0.582 / 0.465–0.518 ms。process全期間peak RSSは5.000→5.188 MiBとなった。runはVM validation・compile・入力・収集を含み、探索単独ではない。要求量をRSSや物理copy量と同一視しない。多数一致の一括Vec収集を有界Builderへ替えた効果は観測した条件へ限定する。

[通常cached batch原本](regex_collection_batch_2026_10_15.jsonl)・[集計](regex_collection_batch_2026_10_15.summary.json)は、別のRune release binaryをCargo artifactから保存し、全source / manifest / lib hashを照合した。全sampleがartifact cache hitの72 fresh process。dense 4,096一致のexecute中央値は51.628→51.700 ms、範囲51.449–51.725 / 51.220–53.978 msと重なる。全期間peak RSS中央値は31.320→31.570 MiB。通常入口の速度改善・RSS非増加は確認していない。cold compile、split / capture置換の性能、allocator内部の原因はこの系列の対象外である。
