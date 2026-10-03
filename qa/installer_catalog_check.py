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
print('PASS: initial catalog sync, offline fallback, failure recovery and no duplicate sync with newer setup')
