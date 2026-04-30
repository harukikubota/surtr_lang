# Surtr Const 仕様メモ

`const` は Surtr V1 におけるトップレベル定数宣言である。

- `const NAME = expr`
- `const NAME: Ty = expr`
- `private const NAME = expr`
- `public const NAME = expr`

V1 の制約:

- `const` は file top-level のみ
- 名前は `CAP_PATTERN`
- RHS は primitive literal または lens path alias のみ
- `public const` は compile unit 全体で unqualified 参照可能
- `public const` は `Namespace::NAME` の qualified path でも参照可能
- `private const` は定義 file 内のみ参照可能
- `const` は module/import 名前空間には入らない

定数関数 helper の名前は `const/1` ではなく `always/1` を正称とする。
