"""Bootstrap integrity, rerun/state preservation, dependency reuse/resume. No system changes."""
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import shlex
import subprocess
import sys
import tempfile

binary = Path(sys.argv[1]).resolve()
repo = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="rhyven-installer-") as temporary:
    root = Path(temporary)
    releases = root / "release"
    releases.mkdir()
    system = "macos" if platform.system() == "Darwin" else "linux"
    arch = "aarch64" if platform.machine() in ("arm64", "aarch64") else "x86_64"
    asset = releases / f"rhyven-{system}-{arch}"
    shutil.copy2(binary, asset)
    original = asset.read_bytes()
    (releases / "SHA256SUMS").write_text(hashlib.sha256(original).hexdigest()+"  "+asset.name+"\n")
    private_key = root / "test-key.pem"
    public_key = root / "test-key.pub"
    subprocess.run(["openssl", "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", str(private_key)], check=True, capture_output=True)
    subprocess.run(["openssl", "pkey", "-in", str(private_key), "-pubout", "-out", str(public_key)], check=True)
    subprocess.run(["openssl", "dgst", "-sha256", "-sign", str(private_key), "-out", str(releases / "SHA256SUMS.sig"), str(releases / "SHA256SUMS")], check=True)
    home = root / "state"
    mock = root / "mock"
    mock.mkdir()
    engine = root / "engine"
    starts = root / "starts"
    healthy = dict(OSType="linux",Architecture="x86_64",CpuCfsQuota=True,CpuCfsPeriod=True,MemoryLimit=True,PidsLimit=True,CgroupDriver="systemd",CgroupVersion="2",SecurityOptions=["name=rootless"])
    def executable(name, text):
        p=mock/name; p.write_text("#!/usr/bin/env python3\n"+text); p.chmod(0o755)
    executable("docker",f'''import sys,json
from pathlib import Path
args=sys.argv[1:]
if args[:2]==["context","inspect"]: print("unix:///test/docker.sock")
elif args[:2]==["context","show"]: print("rootless")
elif args[0]=="info":
 if not Path({str(engine)!r}).exists(): sys.exit(1)
 print(json.dumps({healthy!r}))
else: sys.exit(2)
''')
    executable("sudo", "raise SystemExit('Unexpected privileged operation in installer test')\n")
    env = dict(os.environ, RHYVEN_HOME=str(home), PATH=str(mock)+os.pathsep+os.environ["PATH"])
    env.pop("DOCKER_HOST", None); env.pop("DOCKER_CONTEXT", None)
    def cli(*args):
        p=subprocess.run([str(binary),*args],env=env,text=True,capture_output=True,timeout=30)
        assert p.returncode==0,(p.stdout,p.stderr)
        return json.loads(p.stdout)
    assert cli("setup","--plan")["plan"] and not home.exists()
    install = ["bash",str(repo/"scripts/install.sh"),"--from-dir",str(releases),"--bin-dir",str(root/"bin space ' quote"),"--no-modify-path","--public-key",str(public_key)]
    def bootstrap(extra=(), success=True):
        p=subprocess.run(install+list(extra),env=env,text=True,capture_output=True,timeout=45)
        assert (p.returncode==0)==success,(p.stdout,p.stderr)
        return p
    first = bootstrap()
    # Commands must remain copyable even with spaces and quotes in the install path.
    next_steps = [shlex.split(line.strip()) for line in first.stdout.splitlines() if line.startswith("  '")]
    assert next_steps and all(command[0] == str(root/"bin space ' quote/rhyven") for command in next_steps)
    agent_command = next(command for command in next_steps if command[1:] == ["--agent"])
    instructions = subprocess.run(agent_command, env=env, capture_output=True, text=True, timeout=30)
    assert instructions.returncode == 0, instructions.stderr
    assert isinstance(json.loads(instructions.stdout), dict)
    assert "PATH was not changed" in first.stdout
    cli("collection","use","my-project")
    marker=home/"collections/my-project/keep.txt"; marker.write_text("retain me")
    bootstrap()
    assert marker.read_text()=="retain me" and cli("collection","current")["collection"]=="my-project"
    installed=root/"bin space ' quote/rhyven"
    good=installed.read_bytes()
    asset.write_bytes(b"corrupt")
    assert "Checksum mismatch" in bootstrap(success=False).stderr
    assert installed.read_bytes()==good
    asset.write_bytes(original)
    engine.touch()
    bootstrap(["--containers","--yes"])
    assert json.loads((home/"setup-state.json").read_text())["status"]=="ready"
    assert not starts.exists()  # healthy engine reused
    if platform.system()=="Linux":
        # Exercise real embedded shell routing with inert command substitutes.
        engine.unlink()
        executable("id", "import sys\nprint('1000' if sys.argv[1]=='-u' else 'testuser')\n")
        executable("systemctl",f'''import sys
from pathlib import Path
assert sys.argv[1:]==["--user","start","docker"],sys.argv
Path({str(starts)!r}).touch()
Path({str(engine)!r}).touch()
''')
        result=cli("setup","--containers","--yes")
        assert result["status"]=="ready" and starts.exists(),result
        engine.unlink()
        executable("systemctl","raise SystemExit(1)\n")
        assert cli("setup","--containers","--yes")["status"]=="pending"
        engine.touch()
        assert cli("setup","--containers")["status"]=="ready"
    assert marker.read_text()=="retain me"
print("PASS: verified installation, corruption rejection, repeated install, retained collections, engine reuse, dependency startup and resumable pending state")
