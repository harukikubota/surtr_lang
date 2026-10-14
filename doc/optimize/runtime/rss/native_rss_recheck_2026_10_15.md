# OI-037 / OI-038のRSS追試

履歴の製品ソースを復元し、counter無効のfresh processを各条件6回、AB/BA順で比較した。RSS差は再現したが、allocator / OS内部の原因は特定できていない。製品のコピー復活やallocator強制変更は行わない。作業入力は[残り6分野の計画](../remaining_implementation_2026_10_14.md)。

## 来歴と方法

Futureは`adefb5fc` / `282f78151`。VMのSHAは元測定のbefore / afterと一致し、製品差分は`vm.rs`、テスト差分は`future_stack_tests.rs`だけ。HashMapは`547c6a68` / `981464e44`で、afterの共通probeを両方へ置いた。beforeの当時のprobeそのものとのbyte一致は主張しない。今回の数値は再構築した履歴版の比較であり、10月15日の最終mainの性能値ではない。

ソースごとに独立したtarget directoryを使用し、Cargo JSONから指定したexecutableを保存した。初回の共有cacheから旧依存artifactが混入した系列と、複数binの最後を誤選択したFuture系列は、測定前の検証で拒否して除外した。これらの準備失敗を製品のRedや性能値には数えない。復元スクリプトの再実行でも6系列のRustソース一致を確認した。

全内容検証・requested allocation・時間/RSSを別processで実行。Futureのchain/nested HashMapは1,000/10,000要素、Tuple対照は10,000。HashMapは1,024要素のdistinct / map_values / sort、String/Int。defaultのほか、子processに`MallocNanoZone=0`、`MallocSecureAllocator=0`かつNano無効を指定した系列を保持した。環境指定だけでallocator切替え済みとは扱わない。

## OI-037

単位はMiB。以下はdefault環境のprocess全期間peak RSSで、VM区間の保持量ではない。

| 入力 | before中央値（範囲） | after中央値（範囲） |
|---|---:|---:|
| chain / HashMap / 10,000 | 76.414（76.391–76.438） | 98.742（98.734–98.766） |
| nested / HashMap / 10,000 | 87.477（76.266–98.688） | 98.594（98.578–98.609） |
| chain / Tuple / 10,000 | 57.289（54.234–57.359） | 52.633（48.016–54.172） |

defaultの全Future条件・全6区間でrequested / freed / live delta / peak extraは前後一致した。chain 10,000のrun requestedは883,866,151 bytes、nestedは312,024,624 bytesで両版一致。時間中央値はchain 321.312→242.654 ms、nested 140.822→65.304 ms。この時間は手組みVMのobserverを含むrun区間である。

旧binary自身もnestedで高低2群へ分かれる。すべてのpayloadで一定のRSS増加が生じるわけではない。要求layoutが同じであることはRSS差の否定にも、allocatorの特定原因の証明にもならない。環境指定系列にも差が残った。OI-037は未解決として維持する。

## OI-038

| 入力 | before RSS中央値 | after RSS中央値 | before / afterの範囲 |
|---|---:|---:|---|
| VM distinct String | 6.203 | 6.375 | 6.203–6.250 / 6.375–6.406 |
| VM map_values String | 6.422 | 6.531 | 6.359–6.422 / 6.484–6.562 |
| sort String対照 | 3.438 | 3.453 | 3.422–3.453 / 3.438–3.453 |
| sort Int対照 | 3.000 | 3.000 | 両版3.000 |

distinct Stringのrun requestedは282,593,722→3,953,382 bytes、peak extraは2,170,699→2,081,749 bytes、最終live deltaは両版1,481,677 bytes。run時間は25.613→0.927 ms。RSSの中央値増は0.172 MiBで、確保要求の減少と併存する。sort対照で同じ増分は確認していない。環境指定系列でもVMの差は残り、OI-037と同じ原因だと決める根拠はない。OI-038も未解決として維持する。

再現スクリプトは[手動測定harness](../../../../tests/profile/runtime_rss_recheck/README.md)、原本とsource / binary / runner SHAは[証跡](rss_evidence_2026_10_15/validation.json)へ保存した。configを要求したstderrは全件空であり、allocator切替えは確認できていない。

## 同じsourceの別artifact対照

汎用buildスクリプトでbeforeの同じ記録入力をreplayした。source SHAとfeatureは一致したが、binary SHAの完全一致は得られなかった。commandには`/tmp`と`/private/tmp`のpath表記差もある。同じ物理rootとtargetであり、SHA差の原因をpathだけへ断定しない。元の測定binaryは保存したまま、別artifactとして検証・測定した。

両artifactの全内容検証8 process、時間/RSS各6回AB/BAと独立countを合わせた28測定processで、全6区間のrequested / freed / live delta / peak extraが一致した。chain 10,000のRSS中央値は76.422 / 76.359 MiBで、高い側は両方約98 MiBへ達した。nestedの中央値は両方76.266 MiB。sourceが一致してもbinaryを同一と呼ばず、元の保存binaryとの来歴を分ける。この対照でもOI-037の原因特定には至っていない。
