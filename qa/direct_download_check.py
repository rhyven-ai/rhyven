"""Real binary installation through local HTTPS; no public downloads or system changes."""
import functools
import hashlib
import http.server
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import ssl
import subprocess
import sys
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[1]
binary = Path(sys.argv[1]).resolve()
spec = importlib.util.spec_from_file_location('downloads', ROOT / 'scripts/stage-downloads.py')
downloads = importlib.util.module_from_spec(spec)
spec.loader.exec_module(downloads)


def run(command, *, env=None, success=True):
    result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True, timeout=60)
    assert (result.returncode == 0) == success, (command, result.stdout, result.stderr)
    return result


with tempfile.TemporaryDirectory(prefix='rhyven-direct-download-') as temporary:
    root = Path(temporary)
    artifacts = root / 'artifacts'
    run(['bash', 'scripts/package-release.sh', str(binary), str(artifacts)])
    (artifacts / 'private-source.txt').write_text('Must not be distributed')
    version = (artifacts / 'VERSION').read_text().strip()
    asset = next(artifacts.glob('rhyven-*')).name
    original_hash = downloads.digest(binary)
    public_root = root / 'site'
    public_root.mkdir()
    requests = []

    class Handler(http.server.SimpleHTTPRequestHandler):
        def do_GET(self):
            requests.append(self.path)
            return super().do_GET()

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(
        ('127.0.0.1', 0), functools.partial(Handler, directory=str(public_root)))
    cert = root / 'cert.pem'
    key = root / 'key.pem'
    # Trust this ephemeral certificate only in the test process, never the OS store.
    run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes',
         '-keyout', str(key), '-out', str(cert), '-days', '1',
         '-subj', '/CN=127.0.0.1', '-addext', 'subjectAltName=IP:127.0.0.1'])
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(cert, key)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        base = f'https://127.0.0.1:{server.server_port}/downloads'
        output = public_root / 'downloads'
        staged_version, names = downloads.stage([artifacts], output, base, key)
        assert staged_version == version and names == [asset]
        release = output / 'releases' / f'v{version}'
        expected_files = {'install.sh', 'VERSION', *(f'releases/v{version}/{name}' for name in
                           (asset, 'install.sh', 'VERSION', 'SHA256SUMS', 'SHA256SUMS.sig', 'release-key.pem', 'LICENSE', 'NOTICE', 'THIRD_PARTY_NOTICES.txt'))}
        assert {str(p.relative_to(output)) for p in output.rglob('*') if p.is_file()} == expected_files
        assert (release / 'LICENSE').read_bytes() == (ROOT / 'LICENSE').read_bytes()
        assert (release / 'NOTICE').read_bytes() == (ROOT / 'NOTICE').read_bytes()
        for line in (release / 'SHA256SUMS').read_text().splitlines():
            sha, name = line.split()
            assert downloads.digest(release / name) == sha
        script = (output / 'install.sh').read_text()
        assert f'version=v{version}\n' in script and f'default_download_base_url={base}\n' in script

        mock = root / 'mock'
        mock.mkdir()
        forbidden = root / 'forbidden-command'

        def executable(name, body):
            file = mock / name
            file.write_text('#!/usr/bin/env python3\n' + body)
            file.chmod(0o755)

        for name in ('gh', 'git', 'sudo'):
            executable(name, f'from pathlib import Path\nPath({str(forbidden)!r}).touch()\nraise SystemExit(90)\n')
        healthy = dict(OSType='linux', Architecture=platform.machine(), CpuCfsQuota=True,
                       CpuCfsPeriod=True, MemoryLimit=True, PidsLimit=True,
                       CgroupDriver='systemd', CgroupVersion='2', SecurityOptions=['name=rootless'])
        executable('docker', f'''import sys, json
args = sys.argv[1:]
if args[:2] == ['context', 'inspect']: print('unix:///test/docker.sock')
elif args[:2] == ['context', 'show']: print('rootless')
elif args[0] == 'info': print(json.dumps({healthy!r}))
else: raise SystemExit(91)
''')
        home = root / 'state'
        bin_dir = root / "bin space ' quote"
        env = dict(os.environ, RHYVEN_HOME=str(home), CURL_CA_BUNDLE=str(cert),
                   PATH=os.pathsep.join((str(bin_dir), str(mock), os.environ['PATH'])))
        for key_name in ('DOCKER_HOST', 'DOCKER_CONTEXT', 'RHYVEN_DOWNLOAD_BASE_URL'):
            env.pop(key_name, None)
        pipeline = ['bash', '-o', 'pipefail', '-c',
                    'curl -fsSL "$1/install.sh" | bash -s -- --containers --bin-dir "$2" --no-modify-path',
                    'download-test', base, str(bin_dir)]
        prior = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else None
        record = None
        if prior:
            prior_version = run([str(prior), '--version']).stdout.strip()
            assert prior_version.startswith('rhyven ') and prior_version != f'rhyven {version}'
            bin_dir.mkdir(parents=True,exist_ok=True)
            shutil.copyfile(prior,bin_dir/'rhyven');(bin_dir/'rhyven').chmod(0o755)
            fixture=ROOT/'crates/core/tests/fixtures/work-management-0.3.0.json'
            run(['rhyven','install',str(fixture),'--accept-permissions'],env=env)
            record=json.loads(run(['rhyven','call','create',json.dumps({'app':'official/work-management','object':'task','data':{'title':'Preserve across 0.5 upgrade'}})],env=env).stdout)
        run(pipeline, env=env)
        installed = bin_dir / 'rhyven'
        if record:
            recovered=json.loads(run(['rhyven','call','get',json.dumps({'app':'official/work-management','object':'task','id':record['id']})],env=env).stdout)
            assert recovered==record, (recovered,record)
        assert run(['rhyven', '--version'], env=env).stdout.strip() == f'rhyven {version}'
        setup = json.loads((home / 'setup-state.json').read_text())
        assert setup['status'] == 'ready'
        skill_report = json.loads(run(['rhyven', 'skills'], env=env).stdout)
        assert len(skill_report['files']) == 7
        for item in skill_report['files']:
            assert item['status'] == 'installed'
            assert Path(item['path']).read_bytes() == (ROOT / 'skills' / item['name']).read_bytes()
        assert setup['skills']['directory'] == str(home / 'skills' / version)
        # A non-TTY invocation of the same launch command returns agent connection instructions.
        instructions = json.loads(run(['rhyven'], env=env).stdout)
        assert instructions and 'connect' in json.dumps(instructions)
        assert requests == ['/downloads/install.sh', f'/downloads/releases/v{version}/SHA256SUMS', f'/downloads/releases/v{version}/SHA256SUMS.sig',
                            f'/downloads/releases/v{version}/{asset}'], requests

        run(['rhyven', 'collection', 'use', 'retained-project'], env=env)
        marker = home / 'collections/retained-project/keep.txt'
        marker.write_text('Retain this app state')
        # Source installer supports explicit mirrors and a single resolution of latest.
        requests.clear()
        install = ['bash', str(ROOT / 'scripts/install.sh'), '--download-base-url', base,
                   '--bin-dir', str(bin_dir), '--no-modify-path', '--public-key', str(release / 'release-key.pem')]
        run(install, env=env)
        assert requests == ['/downloads/VERSION', f'/downloads/releases/v{version}/SHA256SUMS', f'/downloads/releases/v{version}/SHA256SUMS.sig',
                            f'/downloads/releases/v{version}/{asset}'], requests
        assert json.loads(run(['rhyven', 'collection', 'current'], env=env).stdout)['collection'] == 'retained-project'

        manifest = (release / 'SHA256SUMS').read_bytes()
        signature = (release / 'SHA256SUMS.sig').read_bytes()
        (release / 'SHA256SUMS').write_text('0' * 64 + '  ' + asset + '\n')
        assert 'signature verification failed' in run(pipeline, env=env, success=False).stderr
        (release / 'SHA256SUMS').write_bytes(manifest)
        (release / 'SHA256SUMS.sig').write_bytes(b'invalid signature')
        assert 'signature verification failed' in run(pipeline, env=env, success=False).stderr
        (release / 'SHA256SUMS.sig').write_bytes(signature)
        assert downloads.digest(installed) == original_hash
        (release / asset).write_bytes(b'corrupted download')
        assert 'Checksum mismatch' in run(pipeline, env=env, success=False).stderr
        assert downloads.digest(installed) == original_hash
        shutil.copyfile(binary, release / asset)
        (output / 'VERSION').write_text('../../invalid\n')
        assert 'Invalid published VERSION' in run(install, env=env, success=False).stderr
        (output / 'VERSION').write_text(version + '\n')
        (release / asset).unlink()
        assert 'Release download failed' in run(pipeline, env=env, success=False).stderr
        assert downloads.digest(installed) == original_hash and marker.read_text() == 'Retain this app state'
        run(install + ['--download-base-url', base.replace('https:', 'http:')], env=env, success=False)
        run(install + ['--version', '../invalid'], env=env, success=False)
        assert not forbidden.exists(), 'Installer tried GitHub, Git or privileged system changes'

        def reject(artifacts_list, destination, origin, error):
            try:
                downloads.stage(artifacts_list, destination, origin, key)
            except ValueError as failure:
                assert error in str(failure), str(failure)
            else:
                raise AssertionError('Expected staging to reject invalid input')

        reject([artifacts], output, base, 'already exists')
        reject([artifacts], root / 'bad-url', 'http://example.com', 'HTTPS')
        other = root / 'other'
        shutil.copytree(artifacts, other)
        (other / 'VERSION').write_text('9.9.9\n')
        reject([artifacts, other], root / 'bad-version', base, 'same valid VERSION')
        (other / 'VERSION').write_text(version + '\n')
        (other / asset).write_bytes(b'bad bytes')
        reject([other], root / 'bad-checksum', base, 'Checksum mismatch')
        (other / 'SHA256SUMS').write_text(f'{hashlib.sha256(b"bad bytes").hexdigest()}  {asset}\n')
        reject([artifacts, other], root / 'conflict', base, 'Conflicting binaries')
        (other / asset).unlink()
        (other / asset).symlink_to(binary)
        reject([other], root / 'symlink', base, 'regular file')
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)

print('PASS: staged allowlisted release; real HTTPS install and launch; dependency reuse; no GitHub access; '
      'pinned/latest versions; retained collection; checksum, missing asset and invalid version rejection; '
      'staging conflicts rejected. Public domain was not contacted.')
