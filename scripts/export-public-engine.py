#!/usr/bin/env python3
"""Export the reviewed source inventory without development history or local state."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import tempfile

ROOT = Path(__file__).resolve().parents[1]
INVENTORY = ROOT / 'packaging/public-source-files.json'
BLOCKED = re.compile(
    r'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|'
    r'\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16})\b|'
    r'/(?:home|Users|mnt/[a-z]/Users)/scorn[^/\s]*/', re.IGNORECASE)


def relative_file(name):
    path = PurePosixPath(name)
    if path.is_absolute() or '..' in path.parts or '\\' in name or not path.parts:
        raise ValueError('Invalid source inventory path')
    if any(part in {'.git', '.release-signing', '.rhyven', '.wrangler', 'website', 'target', 'dist', '.env'} for part in path.parts):
        raise ValueError('Local state is not source')
    if path.suffix in {'.pem', '.key', '.sqlite3', '.db', '.zip'}:
        raise ValueError('Unexpected source file type')
    return path


def export(source, output, selected=None):
    source = source.resolve()
    if output.exists():
        raise ValueError('Output already exists; choose a fresh source directory')
    selected = selected if selected is not None else json.loads(INVENTORY.read_text())
    contents = {}
    for destination, relative in sorted(selected.items()):
        relative_file(destination)
        relative_file(relative)
        path = source / relative
        if not path.is_file() or any(p.is_symlink() for p in (path, *path.parents)):
            raise ValueError(f'Expected regular source file: {relative}')
        data = path.read_bytes()
        if len(data) > 20 * 1024 * 1024:
            raise ValueError(f'Unexpected source file size: {relative}')
        if BLOCKED.search(data.decode('utf-8', errors='replace')):
            raise ValueError(f'Credential or personal path candidate in {relative}; review locally')
        contents[destination] = (data, path.stat().st_mode & 0o111)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.public-engine-', dir=output.parent) as temporary:
        tree = Path(temporary) / 'source'
        tree.mkdir()
        for destination, (data, executable) in contents.items():
            target = tree / destination
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            target.chmod(0o755 if executable else 0o644)
        inventory = {name: hashlib.sha256(data).hexdigest() for name, (data, _) in contents.items()}
        (tree / 'SOURCE-MANIFEST.json').write_text(json.dumps({
            'format': 1, 'license': 'Apache-2.0', 'files': inventory}, indent=2) + '\n')
        tree.rename(output)
    return len(contents)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    try:
        count = export(ROOT, args.out)
    except (ValueError, OSError) as error:
        parser.exit(1, f'Source export refused: {error}\n')
    print(f'Exported {count} reviewed source files and their hashes. Nothing published.')
