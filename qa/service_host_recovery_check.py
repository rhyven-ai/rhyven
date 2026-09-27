"""Real service with an injected Docker-client outage; actual daemon loss is tested in the VM."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

binary = str(Path(sys.argv[1]).resolve())
image = Path(sys.argv[2]).read_text().strip()
app = "example/background-counter"
docker = shutil.which("docker")
assert docker

with tempfile.TemporaryDirectory(prefix="rv-host-") as temporary:
    home = Path(temporary)
    offline = home / "offline"
    wrappers = home / "bin"
    wrappers.mkdir()
    wrapper = wrappers / "docker"
    wrapper.write_text("#!/usr/bin/env python3\nimport os,sys\nfrom pathlib import Path\n"
                       f"if Path({str(offline)!r}).exists():\n"
                       " print('Test outage: cannot connect to Docker',file=sys.stderr)\n sys.exit(1)\n"
                       f"os.execv({docker!r},[{docker!r}]+sys.argv[1:])\n")
    wrapper.chmod(0o755)
    environment = dict(os.environ, PATH=str(wrappers) + os.pathsep + os.environ["PATH"])

    def cli(*args, error=False):
        p = subprocess.run([binary, "--home", str(home), *args], env=environment,
                           capture_output=True, text=True, timeout=120)
        if error:
            assert p.returncode, p.stdout
            return json.loads(p.stderr)
        assert not p.returncode, (args, p.stderr)
        return json.loads(p.stdout)

    def status():
        return cli("service", "status", app)

    def eventually(predicate, seconds=35):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if predicate():
                return
            time.sleep(.2)
        raise AssertionError(status())

    p = json.loads(Path("examples/container-service-python/app.json").read_text())
    p["execution"].update(image=image, restart_limit=0)
    package = home / "app.json"
    package.write_text(json.dumps(p))
    try:
        cli("install", str(package), "--accept-permissions")
        cli("daemon", "start")
        cli("service", "start", app)
        before = status()["generation"]
        offline.touch()
        cli("daemon", "stop")
        eventually(lambda: not (home / "supervisor/control.sock").exists())
        cli("daemon", "start")
        eventually(lambda: status()["state"] == "unavailable")
        time.sleep(6)  # Longer than the host retry delay; no app restart budget consumed.
        assert status().get("restart_count", 0) == 0, status()
        offline.unlink()
        eventually(lambda: status()["state"] == "ready")
        assert status()["generation"] != before
        assert status().get("restart_count", 0) == 0

        # Failed maintenance is not automatically resumed, even across daemon restart.
        offline.touch()
        cli("backup", "global", "--out", str(home / "failed.rhyven"), error=True)
        assert status()["suspended"] is True
        cli("backup", "global", "--out", str(home / "retry-failed.rhyven"), error=True)
        assert not (home / "retry-failed.rhyven").exists()
        cli("daemon", "stop")
        eventually(lambda: not (home / "supervisor/control.sock").exists())
        offline.unlink()
        cli("daemon", "start")
        time.sleep(2)
        assert status()["suspended"] is True, status()
        cli("service", "start", app)
        eventually(lambda: status()["state"] == "ready")

        cli("service", "stop", app)
        offline.touch()
        cli("daemon", "stop")
        eventually(lambda: not (home / "supervisor/control.sock").exists())
        cli("daemon", "start")
        offline.unlink()
        time.sleep(2)
        assert status()["desired"] == "stopped" and status()["explicitly_stopped"] is True
        assert status()["state"] == "stopped", status()
        print("PASS: shutdown with Docker unavailable; delayed host recovery without app retries; orphan fencing; backup retries verify shutdown; maintenance remains suspended; explicit stop retained")
    finally:
        offline.unlink(missing_ok=True)
        subprocess.run([binary, "--home", str(home), "service", "stop", app],
                       env=environment, capture_output=True, timeout=120)
        subprocess.run([binary, "--home", str(home), "daemon", "stop"],
                       env=environment, capture_output=True, timeout=120)
