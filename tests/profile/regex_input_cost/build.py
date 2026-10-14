#!/usr/bin/env python3
"""Save a native Regex ownership probe, Cargo artifacts and the exact build inputs."""
import argparse
import hashlib
import json
import shutil
import subprocess
from pathlib import Path


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--revision', required=True)
    parser.add_argument('--target-dir', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--time-only', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    package = root / 'tests/profile/regex_input_cost'
    args.output_dir.mkdir(parents=True, exist_ok=False)
    paths = [root / 'Cargo.toml', root / 'Cargo.lock', root / 'crates/sindr/Cargo.toml',
             package / 'Cargo.toml', package / 'Cargo.lock', package / 'src/main.rs']
    paths.append(package / 'src/metrics.rs')
    for crate in ['sindr', 'eldr', 'diagnostics']:
        if crate != 'sindr':
            paths.append(root / 'crates' / crate / 'Cargo.toml')
        paths.extend(sorted((root / 'crates' / crate / 'src').rglob('*.rs')))
    hashes = {str(path.relative_to(root)): sha(path) for path in paths}
    for path in paths:
        saved = args.output_dir / 'source' / path.relative_to(root)
        saved.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, saved)
    command = ['cargo', 'build', '--locked', '--release', '--manifest-path', str(package / 'Cargo.toml'),
               '--message-format=json', '--target-dir', str(args.target_dir.resolve())]
    if args.time_only:
        command.append('--no-default-features')
    result = subprocess.run(command, cwd=root, capture_output=True, text=True)
    (args.output_dir / 'cargo.jsonl').write_text(result.stdout)
    (args.output_dir / 'compiler.log').write_text(result.stderr)
    result.check_returncode()
    assert hashes == {str(path.relative_to(root)): sha(path) for path in paths}
    artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
    artifact = next(row for row in artifacts if row.get('reason') == 'compiler-artifact'
                    and row['target']['name'] == 'regex-input-cost-probe' and row.get('executable'))
    assert Path(artifact['manifest_path']).resolve() == package / 'Cargo.toml'
    sindr = next(row for row in artifacts if row.get('reason') == 'compiler-artifact' and row['target']['name'] == 'sindr')
    assert Path(sindr['manifest_path']).resolve() == root / 'crates/sindr/Cargo.toml'
    binary = (args.output_dir / 'regex-probe').resolve()
    shutil.copy2(artifact['executable'], binary)
    eldr = next(row for row in artifacts if row.get('reason') == 'compiler-artifact' and row['target']['name'] == 'eldr')
    assert Path(eldr['manifest_path']).resolve() == root / 'crates/eldr/Cargo.toml'
    metadata = dict(head=args.revision, source_root=str(root), command=command,
                    artifact=artifact, sindr_artifact=sindr, eldr_artifact=eldr, binary=str(binary), binary_sha256=sha(binary),
                    source_sha256=hashes, allocator_counting=not args.time_only,
                    builder_sha256=sha(Path(__file__)), rustc_version=subprocess.check_output(['rustc', '-Vv'], text=True))
    (args.output_dir / 'builds.json').write_text(json.dumps(metadata, indent=2) + '\n')
    print(binary)


if __name__ == '__main__':
    main()
