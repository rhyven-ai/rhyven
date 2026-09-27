#!/usr/bin/env python3
"""Assemble a static HTTPS download tree from verified build artifacts. Never publishes."""
import argparse
import hashlib
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
ASSETS = {
    'rhyven-linux-x86_64', 'rhyven-linux-aarch64',
    'rhyven-macos-x86_64', 'rhyven-macos-aarch64',
}
BASE_URL = re.compile(r'https://[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?(?::[0-9]+)?(?:/[A-Za-z0-9._~/-]*)?')
VERSION = re.compile(r'[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?')


def digest(path):
    sha = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            sha.update(chunk)
    return sha.hexdigest()


def stage(artifacts, output, base_url, signing_key):
    if not BASE_URL.fullmatch(base_url):
        raise ValueError('Download base URL must be HTTPS, without credentials, queries or fragments')
    base_url = base_url.rstrip('/')
    if output.exists():
        raise ValueError('Output already exists; choose a new staging directory')
    version = None
    selected = {}
    for folder in artifacts:
        declared = (folder / 'VERSION').read_text().strip()
        if not VERSION.fullmatch(declared) or (version is not None and version != declared):
            raise ValueError('All artifacts must declare the same valid VERSION')
        version = declared
        checksums = {}
        for line in (folder / 'SHA256SUMS').read_text().splitlines():
            parts = line.split()
            if len(parts) != 2 or not re.fullmatch(r'[0-9a-fA-F]{64}', parts[0]):
                raise ValueError(f'Invalid checksum entry in {folder}')
            if parts[1] in checksums:
                raise ValueError(f'Duplicate checksum entry: {parts[1]}')
            checksums[parts[1]] = parts[0].lower()
        found = False
        for name in sorted(ASSETS):
            file = folder / name
            if not file.exists():
                continue
            found = True
            if file.is_symlink() or not file.is_file():
                raise ValueError(f'Binary must be a regular file: {name}')
            actual = digest(file)
            if checksums.get(name) != actual:
                raise ValueError(f'Checksum mismatch: {name}')
            if name in selected and selected[name][1] != actual:
                raise ValueError(f'Conflicting binaries for {name}')
            selected[name] = (file, actual)
        if not found:
            raise ValueError(f'No supported platform binaries in {folder}')

    if not selected:
        raise ValueError('At least one platform artifact is required')
    script = (ROOT / 'scripts/install.sh').read_text()
    if script.count('version=latest\n') != 1 or script.count('default_download_base_url=\n') != 1:
        raise ValueError('Installer template changed; review version and download URL substitution')
    script = script.replace('version=latest\n', f'version=v{version}\n', 1)
    script = script.replace('default_download_base_url=\n', f'default_download_base_url={base_url}\n', 1)
    public_key = subprocess.check_output(['openssl', 'pkey', '-in', str(signing_key), '-pubout'], text=True)
    if not re.fullmatch(r'-----BEGIN PUBLIC KEY-----\n[A-Za-z0-9+/=\n]+-----END PUBLIC KEY-----\n', public_key):
        raise ValueError('Signing key must produce a PEM public key')
    if script.count('embedded_public_key=\n') != 1:
        raise ValueError('Installer public key substitution changed')
    script = script.replace('embedded_public_key=\n', f"embedded_public_key='{public_key.rstrip()}'\n", 1)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.rhyven-downloads-', dir=output.parent) as temporary:
        tree = Path(temporary) / 'public'
        release = tree / 'releases' / f'v{version}'
        release.mkdir(parents=True)
        for name, (file, expected) in selected.items():
            target = release / name
            shutil.copyfile(file, target)
            if digest(target) != expected:
                raise ValueError(f'Binary changed during staging: {name}')
            target.chmod(0o755)
        (release / 'VERSION').write_text(version + '\n')
        (release / 'install.sh').write_text(script)
        (release / 'install.sh').chmod(0o755)
        (release / 'release-key.pem').write_text(public_key)
        notices = sorted(p for p in (ROOT / 'third-party-notices').rglob('*') if p.is_file())
        if not notices or any(p.is_symlink() for p in notices):
            raise ValueError('Regular third-party license files are required')
        (release / 'THIRD_PARTY_NOTICES.txt').write_text('\n\n'.join(
            f'===== {p.relative_to(ROOT / "third-party-notices")} =====\n{p.read_text()}' for p in notices))
        for name in ('LICENSE', 'NOTICE'):
            path = ROOT / name
            if not path.is_file() or path.is_symlink():
                raise ValueError(f'Regular {name} file is required')
            shutil.copyfile(path, release / name)
        names = [*sorted(selected), 'install.sh', 'VERSION', 'release-key.pem', 'LICENSE', 'NOTICE', 'THIRD_PARTY_NOTICES.txt']
        (release / 'SHA256SUMS').write_text(''.join(f'{digest(release / name)}  {name}\n' for name in names))
        subprocess.run(['openssl', 'dgst', '-sha256', '-sign', str(signing_key),
                        '-out', str(release / 'SHA256SUMS.sig'), str(release / 'SHA256SUMS')], check=True)
        subprocess.run(['openssl', 'dgst', '-sha256', '-verify', str(release / 'release-key.pem'),
                        '-signature', str(release / 'SHA256SUMS.sig'), str(release / 'SHA256SUMS')], check=True, stdout=subprocess.DEVNULL)
        shutil.copyfile(release / 'install.sh', tree / 'install.sh')
        shutil.copyfile(release / 'VERSION', tree / 'VERSION')
        tree.rename(output)
    return version, sorted(selected)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base-url', required=True, help='Public HTTPS directory containing install.sh and releases/')
    parser.add_argument('--out', type=Path, required=True, help='New local output directory')
    parser.add_argument('--signing-key', type=Path, required=True, help='Offline maintainer private PEM key; never copied to output')
    parser.add_argument('artifacts', type=Path, nargs='+', help='Directories produced by package-release.sh')
    args = parser.parse_args()
    try:
        version, names = stage(args.artifacts, args.out, args.base_url, args.signing_key)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'Cannot stage downloads: {error}\n')
    print(f'Staged v{version}: {", ".join(names)}\nLocal output: {args.out}\nNothing was uploaded.')
    print(f'After publishing: curl -fsSL {args.base_url.rstrip("/")}/install.sh | bash -s -- --containers')
