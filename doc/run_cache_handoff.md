# `surtr run` cache handoff

## Summary

`surtr run <file.srt>` now has a default-on on-disk `.eldr` cache so repeated script runs can skip the standard-library compile path. The cache is intentionally scoped to source execution; `surtr run <file.eldr>` remains unchanged.

Measured locally on `examples/guess.srt` with `target/debug/surtr`:

- First run: about `1.76s`
- Second run: about `0.10s`

## Implemented behavior

- Cache lookup happens after `CompileSources` is built and before `compile_source`.
- Cache hit decodes the saved `.eldr` and executes it directly.
- Cache miss compiles normally, then stores the resulting bytecode.
- Decode failure or corrupt cache silently removes the bad entry and falls back to compile.
- `SURTR_RUN_CACHE=0`, `false`, `FALSE`, `no`, or `NO` disables cache read/write.
- `SURTR_RUN_CACHE_DIR` overrides the cache directory.
- Without override, cache files are stored under `target/run-cache/eldr` when the running executable is under `target/debug` or `target/release`; otherwise the fallback is the system temp directory.

## Cache key inputs

The key includes:

- cache format version
- current executable fingerprint
- execution command and compile unit kind
- selected / normalized entrypoint
- user file name, pseudo module path, and user source hash
- every staged module file name, module path, source kind, and source hash

CLI args and stdin are intentionally excluded because they do not affect compilation.

## Verification

Completed:

- `cargo fmt --check`
- `cargo check -p rune`
- `cargo test -p rune run_source_cache --test integration`
- `cargo test -p rune run_source_uses_run_cache_on_repeated_invocation --test integration`
- `cargo nextest run -p rune`

`cargo nextest run -p rune` passed `144/144`.

## Follow-up candidates

- Add an explicit cache management command if users need inspection or pruning.
- Consider applying the same cache strategy to `check` or `dump <entry.srt>` if repeated interactive use needs it.
- Separately optimize the first-run stdlib snapshot path in Scar; this cache mainly improves repeated runs.
