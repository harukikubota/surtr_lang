# Forge Test Prelude Cache

## 目的

- `003_compile_time_followup_profile.md` で見た Scar と同じ標準 prelude 再構築コストが Forge tests にもあるか確認する
- 低リスクな `#[cfg(test)]` helper cache で、Forge unit tests の待ち時間を少し短縮する

## 実装日

- 2026-04-29 (Asia/Tokyo)

## 変更点

- `crates/forge/src/lib.rs` の test helper に `OnceLock` cache を追加
- 標準 module stages と `DeclarationIndex` をテストプロセス内で共有
- production codegen には影響なし

## Before / After

コマンド:

```bash
/usr/bin/time -p cargo nextest run -p forge
```

| case | build | nextest summary | real | user | sys |
|---|---:|---:|---:|---:|---:|
| before | 0.76s | 1.852s | 3.34s | 8.70s | 0.68s |
| after | 0.69s | 1.735s | 3.12s | 8.68s | 0.62s |

## 読み取り

- Forge tests は 16 件だけなので、Scar ほど大きな差は出ない
- それでも標準 module parse / precollect の再実行が少し減り、summary と real が微減した
- 次に大きく効く対象は Forge より Rune integration のプロセス起動回数か、Scar の type environment 初期化共有
