#!/usr/bin/env python3
"""Prepare an allowlisted static site with signed downloads. Does not deploy."""
import argparse
import hashlib
import importlib.util
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FILES = (
    'index.html', '404.html', 'app.js', 'docs.js', 'skills.js', 'styles.css', 'data/catalog.json',
    'skills/use-rhyven/SKILL.md', 'skills/use-rhyven/RULE.md',
    'skills/publish-rhyven-app/SKILL.md',
    'skills/build-rhyven-declarative-app/SKILL.md',
    'skills/build-rhyven-container-app/SKILL.md',
    'skills/build-rhyven-service-app/SKILL.md',
    'assets/favicon.svg', 'assets/rhyven-brand.png', 'assets/corvid-crows-pixel.png',
    'assets/fonts/lato-regular.ttf', 'assets/fonts/lato-bold.ttf', 'assets/fonts/LICENSE.txt',
)
spec = importlib.util.spec_from_file_location('security_headers', ROOT / 'website/security_headers.py')
policy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(policy)


def fingerprint_scripts(tree):
    renamed = {}
    for name in ('docs.js', 'skills.js', 'app.js'):
        source = (tree / name).read_text()
        for dependency, target in renamed.items():
            source = source.replace(f"'./{dependency}'", f"'./{target}'")
        data = source.encode()
        target = f'{Path(name).stem}.{hashlib.sha256(data).hexdigest()[:16]}.js'
        (tree / target).write_bytes(data)
        (tree / name).unlink()
        renamed[name] = target
    index = tree / 'index.html'
    html = index.read_text()
    if html.count('src="app.js"') != 1:
        raise ValueError('Expected one website module entry point')
    index.write_text(html.replace('src="app.js"', f'src="{renamed["app.js"]}"'))
    return set(renamed.values())


def deployment_headers(tree, scripts):
    headers = dict(policy.HEADERS, **{'Strict-Transport-Security': 'max-age=31536000'})
    blocks = ['/*\n' + ''.join(f'  {k}: {v}\n' for k, v in headers.items()) +
              '  Cache-Control: no-transform\n']
    # Pages combines matching headers. Keep mutually exclusive cache directives
    # on disjoint paths so immutable releases never inherit no-cache/no-store.
    for name in ['/', '/404', *(f'/{p.relative_to(tree)}' for p in sorted(tree.rglob('*'))
                                if p.is_file() and 'releases' not in p.relative_to(tree).parts)]:
        caching = 'public, max-age=31536000, immutable' if name[1:] in scripts else 'no-store'
        blocks.append(f'{name}\n  Cache-Control: {caching}\n')
    blocks.append('/releases/*\n  Cache-Control: public, max-age=31536000, immutable\n')
    blocks.append('/*.sh\n  Content-Type: text/plain; charset=utf-8\n')
    blocks.append('/skills/*\n  Content-Type: text/plain; charset=utf-8\n')
    return ''.join(blocks)


def stage(downloads, output):
    if output.exists():
        raise ValueError('Output already exists')
    version = (downloads / 'VERSION').read_text().strip()
    if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?', version):
        raise ValueError('Invalid download version')
    release = downloads / 'releases' / f'v{version}'
    mandatory = {'VERSION', 'install.sh', 'SHA256SUMS', 'SHA256SUMS.sig', 'release-key.pem', 'LICENSE', 'NOTICE', 'THIRD_PARTY_NOTICES.txt'}
    optional = {'rhyven-linux-x86_64', 'rhyven-linux-aarch64', 'rhyven-macos-x86_64', 'rhyven-macos-aarch64'}
    actual = {p.name for p in release.iterdir()}
    if not mandatory <= actual or not actual <= mandatory | optional or not actual & optional:
        raise ValueError('Unexpected or incomplete public release files')
    download_files = ['VERSION', 'install.sh', *(f'releases/v{version}/{name}' for name in sorted(actual))]
    if {str(p.relative_to(downloads)) for p in downloads.rglob('*') if p.is_file()} != set(download_files):
        raise ValueError('Unexpected download files; refusing private material')
    if any(p.is_symlink() for p in downloads.rglob('*')):
        raise ValueError('Download symlinks are forbidden')
    # The signer must be reviewed separately; this verifies integrity of the prepared tree.
    subprocess.run(['openssl', 'dgst', '-sha256', '-verify', str(release / 'release-key.pem'),
                    '-signature', str(release / 'SHA256SUMS.sig'), str(release / 'SHA256SUMS')], check=True, stdout=subprocess.DEVNULL)
    checksums = {}
    for line in (release / 'SHA256SUMS').read_text().splitlines():
        sha, name = line.split()
        if name in checksums or name not in actual or not re.fullmatch(r'[0-9a-f]{64}', sha):
            raise ValueError('Invalid signed manifest')
        checksums[name] = sha
    if set(checksums) != actual - {'SHA256SUMS', 'SHA256SUMS.sig'}:
        raise ValueError('Signed manifest must cover every release asset')
    for name, sha in checksums.items():
        if hashlib.sha256((release / name).read_bytes()).hexdigest() != sha:
            raise ValueError(f'Modified release asset: {name}')
    for name in ('install.sh', 'VERSION'):
        if (downloads / name).read_bytes() != (release / name).read_bytes():
            raise ValueError(f'Root {name} differs from signed release')
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.rhyven-site-', dir=output.parent) as temporary:
        tree = Path(temporary) / 'site'
        tree.mkdir()
        for source, names in ((ROOT / 'website', FILES), (downloads, download_files)):
            for name in names:
                src = source / name
                if not src.is_file() or any(p.is_symlink() for p in [src, *src.parents]):
                    raise ValueError(f'Expected regular public file: {name}')
                dst = tree / name
                dst.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(src, dst)
        scripts = fingerprint_scripts(tree)
        (tree / '_headers').write_text(deployment_headers(tree, scripts))
        tree.rename(output)
    return version


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--downloads', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    try:
        version = stage(args.downloads, args.out)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'Cannot stage website: {error}\n')
    print(f'Staged website and v{version} downloads at {args.out}. Nothing uploaded.')
