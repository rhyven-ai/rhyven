#!/usr/bin/env python3
"""Reproducible local security checks. Downloads tools/databases; never uploads source."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
TOOLS = {
    'gitleaks': ('gitleaks/gitleaks', '8.30.1', 'gitleaks_8.30.1_linux_x64.tar.gz', '551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb'),
    'trivy': ('aquasecurity/trivy', '0.74.0', 'trivy_0.74.0_Linux-64bit.tar.gz', '2ae6fe3ee734b7fdf11335663e18c75ea12dccc76062f09f164a3b0f8be4371a'),
}


def install_tools(cache):
    cache.mkdir(parents=True, exist_ok=True)
    for name, (repo, version, archive, digest) in TOOLS.items():
        destination = cache / f'{name}-{version}'
        # Reverify even cached downloads before executing a scanner.
        tarpath = cache / archive
        if not tarpath.exists():
            with urllib.request.urlopen(f'https://github.com/{repo}/releases/download/v{version}/{archive}', timeout=180) as response:
                tarpath.write_bytes(response.read())
        data = tarpath.read_bytes()
        if hashlib.sha256(data).hexdigest() != digest:
            raise ValueError(f'{name} archive checksum mismatch')
        with tarfile.open(fileobj=io.BytesIO(data)) as tar:
            member = next(m for m in tar.getmembers() if m.name in (name, './' + name) and m.isfile())
            destination.write_bytes(tar.extractfile(member).read())
        destination.chmod(0o700)
    return {name: str(cache / f'{name}-{metadata[1]}') for name, metadata in TOOLS.items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=ROOT / 'dist/security-audit')
    parser.add_argument('--cache', type=Path, default=Path('/tmp/rhyven-security-cache'))
    parser.add_argument('--image', action='append', default=[], help='Immutable image reference to scan remotely')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    tools = install_tools(args.cache)
    failed = []

    def run(label, command):
        with (args.out / f'{label}.log').open('w') as log:
            result = subprocess.run(command, cwd=ROOT, stdout=log, stderr=log)
        if result.returncode not in (0, 1):
            raise RuntimeError(f'{label} scanner failed; see local log')

    history = args.out / 'source-history-secrets.json'
    repo = Path(subprocess.check_output(['git', 'rev-parse', '--show-toplevel'], cwd=ROOT, text=True).strip())
    run('history', [tools['gitleaks'], 'git', str(repo), '--redact=100', '--report-format', 'json', '--report-path', str(history), '--log-opts=--all'])
    findings = json.loads(history.read_text())
    if findings:
        failed.append('Source history secrets')
    # Scan current tracked AND untracked nonignored files without caches, local keys or app state.
    with tempfile.TemporaryDirectory(prefix='rhyven-source-scan-') as temporary:
        names = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=repo).decode().split('\0')
        for name in set(names) - {''}:
            src = repo / name
            if src.is_file() and not src.is_symlink():
                dst = Path(temporary) / name
                dst.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(src, dst)
        report = args.out / 'working-tree-secrets.json'
        run('working-tree', [tools['gitleaks'], 'dir', temporary, '--redact=100', '--report-format', 'json', '--report-path', str(report)])
        if json.loads(report.read_text()):
            failed.append('Working tree secrets')
    report = args.out / 'dependencies.json'
    run('dependencies', [tools['trivy'], 'fs', '--scanners', 'vuln', '--skip-dirs', 'target', '--skip-dirs', 'dist',
        '--skip-dirs', 'third-party-notices', '--skip-dirs', 'vendor', '--cache-dir', str(args.cache / 'trivy'),
        '--format', 'json', '--output', str(report), '.'])
    if any(r.get('Vulnerabilities') for r in json.loads(report.read_text()).get('Results', [])):
        failed.append('Dependency vulnerabilities require review')
    for i, reference in enumerate(args.image):
        if '@sha256:' not in reference:
            raise ValueError('Image scan requires an immutable digest')
        report = args.out / f'image-{i}.json'
        run(f'image-{i}', [tools['trivy'], 'image', '--image-src', 'remote', '--scanners', 'vuln,secret',
            '--cache-dir', str(args.cache / 'trivy'), '--format', 'json', '--output', str(report), reference])
        for result in json.loads(report.read_text()).get('Results', []):
            if result.get('Secrets') or any(v['Severity'] in ('HIGH', 'CRITICAL') for v in result.get('Vulnerabilities', [])):
                failed.append(f'Image {i} findings require documented review')
    print(json.dumps({'passed': not failed, 'review_required': sorted(set(failed)), 'reports': str(args.out)}, indent=2))
    return bool(failed)


if __name__ == '__main__':
    raise SystemExit(main())
