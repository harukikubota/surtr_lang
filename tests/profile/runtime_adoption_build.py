#!/usr/bin/env python3
"""Save an ordinary Rune release artifact with exact compiler/runtime/library inputs."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--revision', required=True)
    parser.add_argument('--target-dir', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    paths = [root / 'Cargo.toml', root / 'Cargo.lock', *sorted((root / 'crates').rglob('*.rs')),
             *sorted((root / 'crates').rglob('Cargo.toml')), *sorted((root / 'lib').rglob('*.srt'))]
    hashes = {str(path.relative_to(root)): sha(path) for path in paths}
    for path in paths:
        saved = args.output_dir / 'source' / path.relative_to(root)
        saved.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, saved)
    command = ['cargo', 'build', '--release', '--locked', '-p', 'rune', '--bin', 'surtr',
               '--message-format=json', '--target-dir', str(args.target_dir.resolve())]
    result = subprocess.run(command, cwd=root, text=True, capture_output=True)
    (args.output_dir / 'cargo.jsonl').write_text(result.stdout)
    (args.output_dir / 'compiler.log').write_text(result.stderr)
    result.check_returncode()
    assert hashes == {str(path.relative_to(root)): sha(path) for path in paths}
    records = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
    artifacts = {name: next(row for row in records if row.get('reason') == 'compiler-artifact'
                           and row['target']['name'] == name) for name in ['sindr', 'eldr', 'surtr']}
    for name, artifact in artifacts.items():
        assert Path(artifact['manifest_path']).resolve() == root / 'crates' / ('rune' if name == 'surtr' else name) / 'Cargo.toml'
    binary = (args.output_dir / 'surtr').resolve()
    shutil.copy2(artifacts['surtr']['executable'], binary)
    metadata = dict(revision=args.revision, source_root=str(root), source_sha256=hashes,
                    command=command, artifacts=artifacts, binary=str(binary), binary_sha256=sha(binary),
                    builder_sha256=sha(Path(__file__)), rustc_version=subprocess.check_output(['rustc', '-Vv'], text=True))
    (args.output_dir / 'builds.json').write_text(json.dumps(metadata, indent=2) + '\n')
    print(binary)


if __name__ == '__main__':
    main()
