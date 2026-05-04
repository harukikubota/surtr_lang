#!/usr/bin/env bash
set -euo pipefail

case_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$case_dir/../.." && pwd)"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

run_case() {
  local name="$1"
  local file="$work_dir/$name.srt"

  cat >"$file"

  echo
  echo "===== $name ====="
  (
    cd "$repo_root"
    cargo run -q -p rune -- check "$file"
  ) || true
}

cat <<'INFO'
Hole callable error checks

Note:
- Multi-argument function types in Surtr use `(A, B -> R)`.
- The `Hole` surface marker `_` is only intended for ignored-input callable slots.
INFO

run_case "hole_argument_type_rejected" <<'EOF'
def apply_once(f: (_ -> Int)) -> Int {
  f(())
}
EOF

run_case "hole_data_return_rejected" <<'EOF'
bad: (Int -> _) = const(1)
EOF

run_case "hole_in_container_rejected" <<'EOF'
xs: List<_> = []
EOF

run_case "hole_multi_arity_annotation_rejected" <<'EOF'
def add(x: Int, y: Int) -> Int { x + y }

fn: (_, _ -> Int) = &add
EOF
