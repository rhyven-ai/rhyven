"""Upgrade version checks/failure preservation and requirements without app installation."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

binary = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix='rhyven-polish-') as temp:
    root = Path(temp); mock = root/'mock'; mock.mkdir()
    installed = root/'bin/rhyven'; installed.parent.mkdir(); shutil.copy2(binary,installed)
    before = hashlib.sha256(installed.read_bytes()).hexdigest()
    curl = mock/'curl'
    curl.write_text('''#!/usr/bin/python3
import os,sys
if any(v.endswith('/VERSION') for v in sys.argv): print(os.environ['LATEST'])
else: sys.exit(22)
'''); curl.chmod(0o755)
    env = dict(os.environ, RHYVEN_HOME=str(root/'home'), PATH=str(mock)+os.pathsep+os.environ['PATH'], LATEST='0.4.0-rc.9')
    def cli(*args, ok=True):
        p = subprocess.run([str(installed),*args],env=env,capture_output=True,text=True,timeout=40)
        assert (p.returncode == 0) == ok,(p.stdout,p.stderr)
        return json.loads(p.stdout) if ok else p
    assert cli('upgrade','--check')['update_available'] is False
    version=subprocess.check_output([str(binary),'--version'],text=True).strip().split()[-1]
    major,minor,patch=map(int,version.split('-')[0].split('.'))
    env['LATEST']=f'{major}.{minor}.{patch+1}-rc.1'
    assert cli('upgrade','--check')['update_available'] is True
    env['LATEST']=f'{major}.{minor}.{patch+1}'
    assert cli('upgrade','--check')['update_available'] is True
    env['LATEST']='9.0.0'
    cli('upgrade',ok=False)
    assert hashlib.sha256(installed.read_bytes()).hexdigest() == before
    env['LATEST']='$(touch /tmp/should-not-run)'
    cli('upgrade','--check',ok=False)
    report = cli('call','rhyven_call',json.dumps({'category':'rhyven/marketplace','function':'action_requirements','args':{'app':'rhyven/project-knowledge'}}))
    assert report['status']=='ready',report
    appdir=root/'python-app'; package=root/'python-app.json'
    cli('app','init','test/requirements','--runtime','python','--dir',str(appdir))
    cli('app','package',str(appdir),'--out',str(package))
    cli('app','publish',str(package))
    env['PATH']=str(mock) # no host interpreter can be found; nothing is installed to fix it
    report=cli('call','rhyven_call',json.dumps({'category':'rhyven/marketplace','function':'action_requirements','args':{'app':'test/requirements'}}))
    assert report['status']=='needs_setup',report
    assert 'python3' in report['error']['message'],report
    assert cli('list')==[]
    assert not (root/'home/script-runtime/environments').exists()
print('PASS: prerelease ordering, check-only upgrades, failed replacement preservation, missing Python guidance, no app or dependency installation')
