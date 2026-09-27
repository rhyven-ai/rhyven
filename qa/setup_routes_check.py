"""Exercise missing-engine apt setup with inert system commands; never changes the host."""
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

if platform.system() != "Linux":
    raise SystemExit("SKIP: apt route is Linux-specific")
release = Path('/etc/os-release').read_text()
if not any(x in release.splitlines() for x in ('ID=ubuntu','ID=debian')):
    raise SystemExit("SKIP: automatic apt route supports Ubuntu/Debian")
if any(Path(p).exists() for p in ('/etc/apt/sources.list.d/docker.sources','/etc/apt/sources.list.d/docker.list')):
    raise SystemExit("SKIP: existing Docker source intentionally blocks new source setup")
with tempfile.TemporaryDirectory(prefix='rhyven-apt-route-') as temp:
    root=Path(temp); bins=root/'bin'; bins.mkdir(); log=root/'operations'
    for name in ['uname','grep','mktemp','rm']:
        (bins/name).symlink_to(shutil.which(name))
    def mock(name, code):
        p=bins/name;p.write_text('#!/usr/bin/python3\n'+code);p.chmod(0o755)
    mock('id',"import sys\nprint('1000' if sys.argv[1]=='-u' else 'testuser')\n")
    mock('systemctl','pass\n')
    mock('dpkg-query','raise SystemExit(1)\n')
    mock('dpkg',"print('amd64')\n")
    mock('sudo',f'''import json,sys
from pathlib import Path
with open({str(log)!r},'a') as f: f.write(json.dumps(sys.argv[1:])+'\\n')
''')
    mock('curl',"import sys\nfrom pathlib import Path\nPath(sys.argv[sys.argv.index('-o')+1]).write_text('test-key')\n")
    p=subprocess.run(['/bin/bash','scripts/setup-containers.sh'],env=dict(os.environ,PATH=str(bins),RHYVEN_SETUP_YES='1'),text=True,capture_output=True,timeout=30)
    assert p.returncode==20,(p.returncode,p.stdout,p.stderr)
    operations=[json.loads(line) for line in log.read_text().splitlines()]
    assert ['apt-get','install','-y','docker-ce','docker-ce-cli','containerd.io','docker-buildx-plugin','docker-compose-plugin'] in operations,operations
    assert ['systemctl','enable','--now','docker'] in operations
    assert ['usermod','-aG','docker','testuser'] in operations
    assert 'log out of all sessions or reboot' in p.stderr
    assert 'systemd user services' in p.stderr
print('PASS: approved missing-engine setup initiates signed-repository apt installation, startup and login-resume guidance (mock system commands)')
