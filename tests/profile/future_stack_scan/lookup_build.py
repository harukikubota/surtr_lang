#!/usr/bin/env python3
"""Save the exact Eldr Cargo test executable and its local build inputs."""
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
    parser.add_argument('--target-dir', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--root', type=Path, help='source root, including an archived Git revision')
    parser.add_argument('--revision', help='explicit archived revision; verified by the reproduction patch/source hashes')
    parser.add_argument('--release', action='store_true')
    args = parser.parse_args()
    root = (args.root or Path(__file__).resolve().parents[3]).resolve()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    paths = [root / 'Cargo.toml', root / 'Cargo.lock']
    for crate in ['eldr', 'sindr', 'diagnostics']:
        paths.append(root / 'crates' / crate / 'Cargo.toml')
        paths.extend(sorted((root / 'crates' / crate / 'src').rglob('*.rs')))
    hashes = {str(path.relative_to(root)): sha(path) for path in paths}
    for path in paths:
        saved = args.output_dir / 'source' / path.relative_to(root)
        saved.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, saved)
    command = ['cargo', 'test', '--locked', '-p', 'eldr', '--lib', '--no-run',
               '--message-format=json', '--target-dir', str(args.target_dir.resolve())]
    if args.release:
        command.append('--release')
    result = subprocess.run(command, cwd=root, capture_output=True, text=True)
    (args.output_dir / 'cargo.jsonl').write_text(result.stdout)
    (args.output_dir / 'compiler.log').write_text(result.stderr)
    result.check_returncode()
    assert hashes == {str(path.relative_to(root)): sha(path) for path in paths}
    artifact = next(row for line in result.stdout.splitlines() if line.startswith('{')
                    for row in [json.loads(line)]
                    if row.get('reason') == 'compiler-artifact'
                    and row['target']['name'] == 'eldr' and row.get('executable'))
    assert Path(artifact['manifest_path']).resolve() == root / 'crates/eldr/Cargo.toml'
    binary = (args.output_dir / 'eldr-test').resolve()
    shutil.copy2(artifact['executable'], binary)
    metadata = {
        'scope': 'Eldr internal VM test executable; ' + ('release' if args.release else 'debug'),
        'source_root': str(root),
        'command': command, 'artifact': artifact,
        'binary': str(binary), 'binary_sha256': sha(binary),
        'source_sha256': hashes,
        'head': args.revision or subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip(),
        'builder_sha256': sha(Path(__file__)),
        'rustc_version': subprocess.check_output(['rustc', '-Vv'], text=True),
    }
    (args.output_dir / 'builds.json').write_text(json.dumps(metadata, indent=2) + '\n')
    print(binary)


if __name__ == '__main__':
    main()
