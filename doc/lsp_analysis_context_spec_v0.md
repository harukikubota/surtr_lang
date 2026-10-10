# Surtr LSP Analysis Context Spec v0

Surtr LSP が補完、hover、definition、diagnostics を出すときに、開いている
`.srt` source をどの compile unit 文脈で読むかを固定するための draft 仕様。

---

## 7. UX Contract

LSP client は解析 context を明示的に切り替えられる UI を持つ。

想定操作名は実装側で自由に決めてよいが、少なくとも次の機能を提供する。

- current file を script entry として解析する
- workspace project として解析する
- current definition source を standalone として解析する
- current definition source を既存 script / project context 下で解析する
- context を解除して automatic discovery に戻す

status 表示例。

```text
Surtr: script tests/fixtures/script/pass/foo.srt
Surtr: project /Users/haruca/work/rust/surtr
Surtr: definition under tests/fixtures/script/pass/foo.srt
Surtr: stdlib lib/types/int.srt
Surtr: standalone definition
```

LSP は同一 file に複数 context 候補がある場合、現在選択中の context を優先する。
候補がない場合だけ automatic discovery を使う。

---

## 8. Cache / Invalidation

LSP は context 単位で parse / resolve / typecheck 結果を cache してよい。

cache key には少なくとも次を含める。

- `workspace_root`
- `mode`
- `entry_file`
- `active_file`
- `entry_file` の content hash
- `active_file` の content hash
- `source_kind`
- `runner_args`
- `include_graph` の edge set と include directive span hash
- `module_stages` の file path と content hash
- `stdlib_stage` の version / content hash

次の場合は context を invalidation する。

- script entry の include directive が変わった
- include file の content が変わった
- project runner 設定または runner args が変わった
- `lib/**/*.srt` の標準 definition source が変わった
- active file の selected entry context が変わった

---
