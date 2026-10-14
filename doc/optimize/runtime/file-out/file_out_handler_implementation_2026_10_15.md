# FileOutHandlerの実装と検証

入力は[ユーザーが承認した4群の具体案](../remaining_implementation_2026_10_14.md#fileouthandler-未決規則の具体案)と[修復案](file_out_handler_lifecycle_repair_spec.md)。ProcessRuntimeの正本を確定案へ整合させるlevel4変更。6分野のうち他の3製品改善はそれぞれ独立コミット済みで、FileOutの完了とは数えない。

## 実装順と所有

1. Scarでctxを使うclosureを、最外closureの外の通常BindとPIDのlexical captureへ正規化する。nested closureはinnerからouterへcaptureを伝搬し、ctx getterを呼出し時に残さない。named Defは別境界。Callable / Value fieldやOpcodeを増やさず、既存closure生成・template・capture metadataを使う。
2. 構造化target、canonical sink registry、明示的binding contextを追加する。schedule_invocationの前に指定contextへ切り替える。init・process・Task・requestの論理所有と、VM active lease・checkpoint saved leaseを分ける。contextのClone自体をowner登録にしない。
3. Singleton init・Standby・Worker spawn・Workers/refillを共通取得へ接続し、成功時のprocess移譲と失敗時の一度の解放を固定する。processのbindingをmanaged execution・ReplyLaterへ設定する。
4. 独立Taskは起動時に取得済みbindingとcaptureしたhandler PIDを保持し、Waiting contextなしでも失わない。FIFO要求自体にもactive leaseを持たせ、未開始取消と開始済みwriteを区別する。
5. Stop後の実行回収・shutdown・rollback・VM clone・saved leaseを接続する。最後のactive ownerでflushし、元の失敗を残してcleanup失敗を配送する。表示逆parseとwriteごとのopenを最終経路から削除する。

canonical alias・comma/equalsを含むpath・init前失敗・部分失敗・retry・rename・同じsinkの複数owner・FIFO小予算・取消・flush失敗をEldrで直接固定する。escaping closureはScar/Forgeと通常入口で捕捉を検証する。通常FileOut rename、rollbackの外部bytes、stdio/Null互換性は必要なfixture/integrationで確認する。最終sourceでCI・標準全件と独立レビューを完了するまでFileOutを未完了として追跡する。

## 進捗

Scarの受入テストは、Unitを返すescaping/nested closureが型検査を通る一方、handler PIDのcaptureが0件でctx getterを呼出し側へ残すことをRedで確認した。PIDそのものを返す初稿は既存の依存返却禁止で拒否され、受入のRedには数えない。現在、正規化と直接検証を進めている。資源・queueはまだ未実装。

### 第一段階の結果

Scarの正規化を実装し、2件で外側Bindと両closureのUID一致、同slot重複排除、2slotと普通のprefix capture共存を確認した。通常fixtureで返却したnested closure、Task::call、部分適用、generic helperのPID捕捉を実際の出力まで確認した。generic helper内のctx直接参照は既存規則で拒否されるため、handler側で取得したPIDを明示的に渡す。言語の型規則や例外を増やしていない。

最終CI 2,588件 / 70 binaries・標準1,124件すべて成功。Scar全件は2例目の追加前に395件、最終capture対象2件とfixture bucket 1件も成功した。途中CIはfixture修正中に一回中断し、次のCIはstdout captureの改行期待だけが1件不一致となった。期待値を既存event join方式に合わせた最終CIが全Greenであり、不一致を製品修正やテスト除外で隠していない。[検証記録](handler_capture_validation_2026_10_15.json)。

独立レビューは最外Bind、inner→outer伝搬、named Def境界、Pattern/Facet/MonadFail/DoAlternative/lazy shellの走査、既存captureとarity/localへの接続を確認し、修正必須の指摘なし。Pattern/Facet/MonadFailのctx捕捉は専用実行例ではなくsource監査である。descriptor保持・owner・FIFO・復帰は次段階であり、FileOut全体を完了とは扱わない。
