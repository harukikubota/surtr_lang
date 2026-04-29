# Float 仕様メモ

最終更新: 2026-04-08

## 位置づけ

`Float` は Surtr の現行実装に存在するが、厳密仕様はまだ固定しない。

本ファイルは、`Float` を削除せずに実装を維持しつつ、今後どこを詰めるべきかを切り出すための別紙である。

## 現時点の扱い

- 構文上は基本型として利用できる
- 現行ランタイム実装は Rust `f64` を使う
- 算術、比較、`safe_div` で動作する
- 表示や比較の細部は Rust 実装依存の部分が残っている

## まだ固定しないもの

- `NaN` の等価性と比較の契約
- `Infinity` / `-Infinity` を値として許すか
- `-0.0` の表示と比較
- `safe_div` が `NaN` や非有限値をどう扱うか
- 将来の標準ライブラリが `Float` をどこまで公開 API に含めるか

## 今回の方針

- `Float` 実装は残す
- ただし、正本仕様では「暫定実装」であることを明記する
- 新規の `Float` 依存 API は、契約が確定するまで慎重に追加する
- レビュー上の `Float` 指摘は、本ファイルと `doc/要件定義v9.md` / `doc/open-issues.md` を起点に追う

## 今後の確定項目

1. `safe_div(Float, Float)` の失敗条件を値レベル失敗にするか、非有限値を許容するか決める
2. `==`, `!=`, `<`, `<=`, `>`, `>=` の `Float` 契約を固定する
3. 表示規則を `to_string` / `inspect` とあわせて固定する
4. `spec` と `compile_errors` の fixture を追加する

## 退避した pending case

以前は skipped integration test として、次のケースを先置きしていた。

```surtr
value = safe_div(0.0, 0.0)
print(to_string(value))
```

このテストは `NaN` を表示しても `ZeroDivisionError` にしても通る曖昧な assertion だったため、テストからは外し、本メモの確定項目として管理する。
仕様化時は次のどちらかを明示してから通常の `spec` / `compile_errors` に追加する。

- `safe_div(0.0, 0.0)` を `Ok(NaN)` 相当として扱い、表示規則を固定する
- `safe_div(0.0, 0.0)` を `Err(ZeroDivisionError)` 相当として扱い、非有限値を発生させない
