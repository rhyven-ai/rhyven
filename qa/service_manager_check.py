"""Exercise a real Linux user service manager, without enabling permanent host startup."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

binary = str(Path(sys.argv[1]).resolve())
image = Path(sys.argv[2]).read_text().strip()
app = "example/background-counter"
with tempfile.TemporaryDirectory(prefix="rv-systemd-") as temporary:
    root = Path(temporary)
    home = root / "state"
    unit = "rhyven-acceptance-" + root.name + ".service"
    def cli(*args):
        p = subprocess.run([binary, "--home", str(home), *args], capture_output=True, text=True, timeout=90)
        assert not p.returncode, (args, p.stderr)
        return json.loads(p.stdout)
    def manager(*args, check=True):
        return subprocess.run(["systemctl", "--user", *args], capture_output=True, text=True, check=check, timeout=60)
    def ready():
        end = time.monotonic() + 35
        while time.monotonic() < end:
            status = cli("service", "status", app)
            if status.get("state") == "ready": return status
            time.sleep(.2)
        raise AssertionError(status)
    package = json.loads(Path("examples/container-service-python/app.json").read_text())
    package["execution"]["image"] = image
    path = root / "app.json"
    path.write_text(json.dumps(package))
    cli("install", str(path), "--accept-permissions")
    output = root / unit
    cli("daemon", "unit", "--out", str(output))
    # Test engine settings are supplied as a service-manager environment, not app permissions.
    def escape(text):
        return text.replace("\\", "\\\\").replace('"', '\\"').replace("%", "%%")
    environment = "".join('Environment="' + escape(key + "=" + os.environ[key]) + '"\n' for key in ["PATH", "DOCKER_HOST", "DOCKER_CONTEXT"] if key in os.environ)
    output.write_text(output.read_text().replace("[Service]\n", "[Service]\n" + environment))
    try:
        manager("enable", "--runtime", "--now", str(output))
        for _ in range(100):
            if (home / "supervisor/control.sock").exists(): break
            time.sleep(.1)
        else:
            raise AssertionError(subprocess.check_output(["journalctl", "--user", "-u", unit, "--no-pager", "-n", "30"], text=True))
        cli("service", "start", app)
        before = ready()
        manager("restart", unit)
        after = ready()
        assert before["generation"] != after["generation"]
        cli("service", "stop", app)
        manager("restart", unit)
        assert cli("service", "status", app)["state"] == "stopped"
        print("PASS: generated unit enabled in runtime only; real systemd start/restart; enabled intent resumes; explicit stop persists")
    finally:
        manager("disable", "--runtime", "--now", unit, check=False)
        manager("reset-failed", unit, check=False)
        manager("daemon-reload", check=False)
