"""Signed bootstrap catalog success/failure/offline/modern-setup compatibility; no network."""
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile

repo = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='rhyven-bootstrap-catalog-') as temp:
    root = Path(temp)
    mock = root/'mock'; mock.mkdir()
    release = root/'release'; release.mkdir()
    system = 'macos' if platform.system() == 'Darwin' else 'linux'
    arch = 'aarch64' if platform.machine() in ('aarch64', 'arm64') else 'x86_64'
    asset = release/f'rhyven-{system}-{arch}'
    asset.write_text('''#!/usr/bin/env python3
import json,os,sys
from pathlib import Path
if sys.argv[1:] == ['--version']: print('rhyven 0.4.0-rc.9')
elif sys.argv[1] == 'setup':
 if '--help' not in sys.argv:
  print(json.dumps({'marketplace': {'status': 'synced'}} if os.environ.get('MODERN') else {'status':'ready'}))
elif sys.argv[1] == 'registry-sync':
 assert sys.argv[2:] == ['rhyven-ai/registry','--anonymous']
 Path(os.environ['SYNC_LOG']).write_text('synced')
 if os.environ.get('FAIL_SYNC'):
  print('Unsupported field: future_feature', file=sys.stderr); sys.exit(1)
 print(json.dumps({'packages':9,'installed':False}))
else: sys.exit(2)
''')
    asset.chmod(0o755)
    (release/'VERSION').write_text('0.4.0-rc.9')
    (release/'SHA256SUMS').write_text(hashlib.sha256(asset.read_bytes()).hexdigest()+'  '+asset.name+'\n')
    private = root/'private.pem'; public = root/'public.pem'
    subprocess.run(['openssl','genpkey','-algorithm','RSA','-pkeyopt','rsa_keygen_bits:2048','-out',str(private)],check=True,capture_output=True)
    subprocess.run(['openssl','pkey','-in',str(private),'-pubout','-out',str(public)],check=True,capture_output=True)
    subprocess.run(['openssl','dgst','-sha256','-sign',str(private),'-out',str(release/'SHA256SUMS.sig'),str(release/'SHA256SUMS')],check=True)
    curl = mock/'curl'
    curl.write_text('#!/usr/bin/env python3\nimport sys,shutil\nfrom pathlib import Path\na=sys.argv\nu=next(v for v in a if v.startswith("https://"))\nshutil.copyfile(Path('+repr(str(release))+')/u.rsplit("/",1)[-1],a[a.index("-o")+1])\n')
    curl.chmod(0o755)
    for case in ('success','failure','offline','modern'):
        log = root/(case+'.log')
        env = dict(os.environ, PATH=str(mock)+os.pathsep+os.environ['PATH'],SYNC_LOG=str(log))
        for key in ('RHYVEN_SETUP_OFFLINE','FAIL_SYNC','MODERN'): env.pop(key,None)
        if case == 'failure': env['FAIL_SYNC']='1'
        if case == 'offline': env['RHYVEN_SETUP_OFFLINE']='1'
        if case == 'modern': env['MODERN']='1'
        result = subprocess.run(['bash',str(repo/'scripts/install.sh'),'--download-base-url','https://installer.test','--public-key',str(public),'--bin-dir',str(root/case),'--no-modify-path'],env=env,text=True,capture_output=True,timeout=20)
        assert result.returncode == 0,(case,result.stdout,result.stderr)
        assert log.exists() == (case in ('success','failure'))
        assert 'Open the terminal marketplace' in result.stdout
        if case == 'failure':
            assert 'bundled apps remain available' in result.stderr
            assert 'update Rhyven' in result.stderr
    for profile_name in ('.profile', '.bash_profile', '.bash_login'):
        test_home = root / ('home-' + profile_name.removeprefix('.'))
        test_home.mkdir()
        profile = test_home / profile_name
        profile.write_text('# Existing user settings\nexport RHYVEN_PROFILE_PRESERVED=yes\n')
        (test_home / '.bashrc').write_text('# Existing interactive settings\n')
        bin_dir = test_home / "tools with spaces and 'quote" / 'bin'
        environment = dict(os.environ, HOME=str(test_home), SHELL='/bin/bash',
                           RHYVEN_HOME=str(test_home / 'state'), MODERN='1')
        environment.pop('RHYVEN_SETUP_OFFLINE', None)
        command = ['bash', str(repo/'scripts/install.sh'), '--from-dir', str(release),
                   '--public-key', str(public), '--bin-dir', str(bin_dir)]
        installed = subprocess.run(command, env=environment, text=True, capture_output=True, timeout=20)
        assert installed.returncode == 0, installed.stderr
        # Execute the actual startup lines in clean child shells from HOME.
        for startup in (profile, test_home/'.bashrc'):
            result = subprocess.run(['bash', '--noprofile', '--norc', '-c',
                                     '. "$1"; cd "$2"; rhyven --version',
                                     'profile-test', str(startup), str(test_home)],
                                    env=environment, capture_output=True, text=True, timeout=10)
            assert result.returncode == 0, result.stderr
            assert result.stdout.strip() == 'rhyven 0.4.0-rc.9'
        before = {p: p.read_bytes() for p in (profile, test_home/'.bashrc')}
        subprocess.run(command, env=environment, check=True, capture_output=True, timeout=20)
        assert all(p.read_bytes() == data for p, data in before.items()), 'Reinstall duplicated PATH lines'
        assert 'RHYVEN_PROFILE_PRESERVED=yes' in profile.read_text()
        activation = installed.stdout.split('To use rhyven in this terminal now, run:\n', 1)[1].splitlines()[0].strip()
        result = subprocess.run(['bash', '--noprofile', '--norc', '-c', activation+'\ncd "$1"\nrhyven --version',
                                 'activation-test', str(test_home)], env=environment,
                                capture_output=True, text=True, timeout=10)
        assert result.returncode == 0 and result.stdout.strip() == 'rhyven 0.4.0-rc.9', result.stderr
print('PASS: catalog sync/fallback; Bash startup and current-terminal activation; quoted paths; repeat installs preserve settings')
