#!/usr/bin/env python3
"""Export reviewed app files into a new tree. Never copies Git history or publishes."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PUBLIC = ('README.md', 'LICENSE', 'NOTICE', 'THIRD_PARTY.md', '.gitignore')
GROUPS = {
    'catalog': ('LICENSE', 'NOTICE', 'ci-management.json', 'error-management.json',
                'inventory.json', 'project-knowledge.json', 'work-management.json'),
    'apps': ('LICENSE', 'NOTICE'),
    'apps/messaging': ('LICENSE', 'NOTICE', 'README.md', '.dockerignore', 'Dockerfile',
                       'app.json', 'main.py', 'rhyven_service.py',
                       'tests/test_mailbox.py', 'tests/integration.py'),
    'apps/repo-documentation-tool': (
        'LICENSE', 'NOTICE', 'README.md', 'GUIDE.md', 'TEST-REPORT.md', 'THIRD_PARTY.md',
        '.dockerignore', 'Dockerfile', 'app.json', 'atlas.py', 'generate_manifest.py',
        'install_toolchains.py', 'lsp.py', 'main.py', 'package.json', 'package-lock.json',
        'repository_client.py', 'toolchains.json', 'tests/integration.py', 'tests/test_atlas.py'),
    'examples': ('LICENSE', 'NOTICE', 'remote-inventory.rhyven.json'),
    'examples/container-python': ('LICENSE', 'NOTICE', 'README.md', '.dockerignore',
                                  'Dockerfile', 'app.json', 'main.py'),
    'examples/container-service-python': ('LICENSE', 'NOTICE', 'README.md', '.dockerignore',
                                          'Dockerfile', 'app.json', 'main.py', 'rhyven_service.py'),
}
SKILLS = ('use-rhyven', 'publish-rhyven-app', 'build-rhyven-declarative-app',
          'build-rhyven-container-app', 'build-rhyven-service-app')
# Fail with filenames only. Never echo potentially sensitive matched text.
FORBIDDEN = (
    re.compile(r'(?i)scorn(?:saber|556)|rhyven-market|rhyven-prototype'),
    re.compile(r'(?i)rhyven-development-archive'),
    re.compile(r'(?i)/(?:home|Users|mnt/[a-z]/Users)/[^\s/]+/'),
    re.compile(r'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----'),
    re.compile(r'\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16})\b'),
)


def selected_files():
    result = {name: 'public-apps/' + name for name in PUBLIC}
    for directory, names in GROUPS.items():
        for name in names:
            relative = directory + '/' + name
            result[relative] = relative
    for skill in SKILLS:
        result[f'skills/{skill}/SKILL.md'] = f'skills/{skill}/SKILL.md'
    result['skills/use-rhyven/RULE.md'] = 'skills/use-rhyven/RULE.md'
    return result


def export(source, output):
    if output.exists():
        raise ValueError('Output already exists; choose a fresh app-only tree')
    source = source.resolve()
    selected = selected_files()
    contents = {}
    for destination, relative in sorted(selected.items()):
        path = source / relative
        if not path.is_file() or any(p.is_symlink() for p in [path, *path.parents]):
            raise ValueError(f'Expected regular app file: {relative}')
        data = path.read_bytes()
        if len(data) > 2 * 1024 * 1024:
            raise ValueError(f'Unexpected large app source: {relative}')
        text = data.decode('utf-8')
        if any(pattern.search(text) for pattern in FORBIDDEN):
            raise ValueError(f'Private reference or credential candidate in {relative}; review locally')
        if destination.endswith('/SKILL.md') and len(text.splitlines()) > 500:
            raise ValueError(f'Skill exceeds 500 lines: {relative}')
        if path.suffix == '.py' or path.name == 'Dockerfile':
            if 'SPDX-License-Identifier: Apache-2.0' not in text:
                raise ValueError(f'Missing app license header: {relative}')
        contents[destination] = data
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.public-apps-', dir=output.parent) as temporary:
        tree = Path(temporary) / 'apps'
        tree.mkdir()
        for destination, data in contents.items():
            path = tree / destination
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        # Relative paths and byte hashes only; no host paths or private commit IDs.
        inventory = {path: hashlib.sha256(data).hexdigest() for path, data in sorted(contents.items())}
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
        parser.exit(1, f'App export refused: {error}\n')
    print(f'Exported {count} reviewed app files and a hash inventory to {args.out}. Nothing published.')
