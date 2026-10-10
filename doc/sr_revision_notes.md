# SR-01〜13への回答に基づく改修案・後段タスクメモ

| 項目 | 状況 | 次の作業・根拠 |
|---|---|---|
| SR-07 | 詳細仕様待ち | Pattern→Expr変換範囲と対象表記の確定が必要 |
| SR-11 | 後続処理の整理 | 呼出し先の一意性と型に依存する引数役割の遅延を区別 |

## SR-07: Pattern consumerのフローを見直す

Pattern→Expr変換範囲と対象表記を確定してからフローを設計する。現行のPattern consumer契約は[Pattern / Extractor実装契約](../docs/dev/Pattern_spec.md)に従う。

## SR-11: Extractorの呼出し先は一意

Extractorも通常関数と同じく、呼出し先が一意であることを保証する。再走査などは不要とする。

この前提でExpr／Patternの候補保持と後続処理を整理する。今回、新たな探索・再試行経路は提案しない。
