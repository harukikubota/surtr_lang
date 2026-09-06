#!/bin/sh
set -eu

if check_output=$(cargo run --quiet -p rune --bin surtr -- check tests/profile/stdlib_prewarm.srt); then
  :
else
  check_status=$?
  printf '%s\n' "$check_output" >&2
  exit "$check_status"
fi
