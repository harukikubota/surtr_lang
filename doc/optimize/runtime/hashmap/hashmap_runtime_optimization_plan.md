# HashMap反復構築のコピー削減

入力は[基準値レポート](hashmap_runtime_measurement_2026_10_11.md)と、2026-10-11の改善実施依頼。
level3として、Sindrの共有値とEldr builtinの既存契約を保つ内部最適化を実施する。
callbackの型・評価規則・継続の構造と予算境界は変更しない。

## 採用方式と範囲

完成前のmapを所有する以下の3経路で、公開の永続的insertを反復する経路を削除する。
空mapの所有storageへ直接insertし、完成したmapだけを返す。

1. `HashMapHandle::from_entries`: 所有するkey / Valueを移し、既存entryをcloneせず構築する。
2. `builtin_map_from_entries`: 現行の入力検査・List iteratorを維持し、検査後のentryを局所mapへ追加する。
3. `MapValuesContinuation::resume`: callbackの成功値を途中outputへ追加し、完了時だけ公開する。

入力件数に応じたreserveやcollectは使わない。少数キーへの多数上書きでもtable容量を
入力件数に比例させないため、実際のdistinct数に応じて成長させる。
公開insert / removeは従来どおり元mapを保持する。新たなBuilder型や共有storageを追加しない。

callbackはキー昇順で一度ずつ呼び、空mapでは呼ばない。言語Err値は通常値として保存する。
RuntimeErrorと取消は追加前に伝播し、部分成功や後続callbackの実行を許さない。
通常のcontext切替えは移動、checkpointに必要なCloneは独立storageの複製という契約を維持する。

sort、開始時のclone、引数やiteratorのclone、Valueの物理操作、JSON object変換、
scheduler分割、OS I/Oは対象外。VM全体の実時間上限は主張しない。
標準API、builtin ID、Opcode、VM/schema version、利用者向け文書の意味は変更しない。

## 受入と実装手順

- 所有入力またはcallback結果のString bufferを、後続entry追加時にcloneしないことを直接テストで固定する。2件のdistinct入力なら、旧経路は1件目を生存中にcloneするためbuffer位置が変わり、Redを確認できる。
- duplicate後勝ち、空map、公開更新後の旧値不変、malformed entryのindex付きRuntimeErrorを検証する。
- 非空途中outputを持つ継続のCloneとVM checkpointからの復帰で、別々の追加が独立することを検証する。待機・取消・量子切替えでcallbackと結果を重複させない。
- テストを先に追加して対象Redを確認後、3経路を置換して対象テストをGreenにする。
- 現行仕様の[Eldr VM仕様](../../../../docs/dev/EldrVM_spec.md)へ内部所有と残る未分割範囲を記載する。
- `rtk cargo nextest run -p sindr -p eldr`を実行し、最終差分で`rtk cargo nextest run --profile ci --workspace`と`rtk proxy cargo run -- test --quiet --all`を直列に一度通す。独立Astraレビューを行う。
- 同じ36条件をcount / time各5回、改善後専用binaryと原本名で測る。same-keyの保持・破棄bytes、sortの対照も記録する。旧原本・旧binaryは上書きしない。

clone数の0は対象3経路の反復insertによる既存entryの全cloneだけを指す。
時間の閾値をcorrectness testへ加えず、意味論テストと手動測定を分けて評価する。
