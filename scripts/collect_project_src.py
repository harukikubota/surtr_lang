#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path


def iter_source_files(crate_root: Path) -> list[Path]:
    files: list[Path] = []
    for crate_dir in sorted(crate_root.iterdir()):
        src_dir = crate_dir / "src"
        if not src_dir.is_dir():
            continue
        files.extend(p for p in src_dir.rglob("*.rs") if p.is_file())
    return sorted(files, key=lambda path: path.relative_to(crate_root))


def iter_test_files(repo_root: Path) -> list[Path]:
    files: list[Path] = []
    rune_tests = repo_root / "crates" / "rune" / "tests"
    tests_root = repo_root / "tests"

    if rune_tests.is_dir():
        files.extend(p for p in rune_tests.glob("*.rs") if p.is_file())
    if tests_root.is_dir():
        files.extend(p for p in tests_root.rglob("*") if p.is_file())

    return sorted(files, key=lambda path: path.relative_to(repo_root))


def build_tagged_file(paths: list[Path], root: Path) -> str:
    chunks: list[str] = []
    for path in paths:
        relative_path = path.relative_to(root)
        content = path.read_text(encoding="utf-8")
        chunks.append(f"#! {relative_path}\n{content.rstrip()}\n")
    return "\n".join(chunks)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Combine repository files into a tagged text dump"
    )
    parser.add_argument(
        "--kind",
        choices=("src", "tests"),
        default="src",
        help="Which file set to collect",
    )
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="Repository root containing the crates directory",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help="Output file path (defaults depend on --kind)",
    )
    args = parser.parse_args()

    repo_root = args.root.resolve()
    crate_root = repo_root / "crates"
    if not crate_root.is_dir():
        raise SystemExit(f"crates directory not found: {crate_root}")

    if args.kind == "src":
        paths = iter_source_files(crate_root)
        default_output = repo_root / "project_src.rs"
        root_for_paths = crate_root.parent
    else:
        paths = iter_test_files(repo_root)
        default_output = repo_root / "project_tests.txt"
        root_for_paths = repo_root

    output_path = args.output.resolve() if args.output else default_output
    output_path.write_text(build_tagged_file(paths, root_for_paths), encoding="utf-8")


if __name__ == "__main__":
    main()
