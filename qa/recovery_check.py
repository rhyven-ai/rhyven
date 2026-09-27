"""Real Docker recovery and migration failure release gate. BINARY PROBE_IMAGE_ID_FILE."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1]).resolve())
image = Path(sys.argv[2]).read_text().strip()
with tempfile.TemporaryDirectory(prefix="rhyven-recovery-") as temporary:
    home = Path(temporary)
    def cli(*args, error=None, collection="global"):
        p = subprocess.run([binary,"--home",str(home),"--collection",collection,*args],text=True,capture_output=True,timeout=90)
        if error:
            assert p.returncode != 0 and json.loads(p.stderr)["code"] == error, (p.stdout,p.stderr)
            return
        assert p.returncode == 0, p.stderr
        return json.loads(p.stdout)
    schema = {"type":"object","properties":{},"required":[],"additionalProperties":False}
    action = {"description":"Recovery test action","input": dict(schema, properties={"from_version":{"type":"string"}}, required=["from_version"]), "output":dict(schema, properties={"ok":{"type":"boolean"}}, required=["ok"])}
    package = {"format":2,"name":"test/recovery","publisher":"test","version":"0.1.0","description":"Recovery fixture","hosting":{"mode":"local"},"execution":{"driver":"container","protocol":"rhyven.container/1","image":image},"permissions":["state.read","state.write","container.execute"],"objects":{},"actions":{"migrate":action,"fail":action,"health":dict(action,input=schema)},"guide":"Recovery fixture","tests":[]}
    path = home / "app.json"
    def install(update=False):
        path.write_text(json.dumps(package))
        return cli("update" if update else "install",str(path),"--accept-permissions")
    install()
    instance = hashlib.sha256(json.dumps(package["name"],separators=(",",":")).encode()).hexdigest()
    data = home / "collections/global/containers" / instance / "data"
    data.mkdir(parents=True)
    (data / "original").write_bytes(b"persistent\x00data")
    (data / "original").chmod(0o700)
    (data / "empty").mkdir()
    package["version"] = "0.2.0"
    package["migrations"] = [{"protocol":1,"from":"0.1.0","action":"migrate"}]
    package["health_action"] = "health"
    result = install(True)
    assert Path(result["recovery_backup"]).exists()
    assert (data / "empty").is_dir() and (data / "original").stat().st_mode & 0o777 == 0o700
    assert json.loads((data / "migration.json").read_text()) == {"from_version":"0.1.0"}
    before = {p.name:p.read_bytes() for p in data.iterdir() if p.is_file()}
    package["version"] = "0.3.0"
    package["migrations"] = [{"protocol":1,"from":"0.2.0","action":"fail"}]
    path.write_text(json.dumps(package))
    cli("update",str(path),"--accept-permissions",error="app_error")
    assert {p.name:p.read_bytes() for p in data.iterdir() if p.is_file()} == before
    assert cli("list")[0]["version"] == "0.2.0"
    backup = home / "state.rhyven"
    cli("backup","global","--out",str(backup))
    cli("restore",str(backup),collection="restored",error="permission_review_required")
    cli("restore",str(backup),"--accept-permissions",collection="restored")
    restored = home / "collections/restored/containers" / instance / "data"
    assert {p.name:p.read_bytes() for p in restored.iterdir() if p.is_file()} == before
    assert (restored / "empty").is_dir() and (restored / "original").stat().st_mode & 0o777 == 0o700
    assert cli("call","rhyven_categories","{}",collection="restored")["collection"] == "restored"
    cli("remove","test/recovery",collection="restored")
    package["version"] = "0.2.0"
    package["migrations"] = [{"protocol":1,"from":"0.1.0","action":"migrate"}]
    path.write_text(json.dumps(package))
    cli("install",str(path),"--accept-permissions",collection="restored")
    assert {p.name:p.read_bytes() for p in restored.iterdir() if p.is_file()} == before
    (data / "escape").symlink_to("/etc/passwd")
    cli("backup","global","--out",str(home / "unsafe.rhyven"),error="backup")
print("PASS: container staged migration, health check, rollback after side effects, backup/restore, reinstall retained data and symlink rejection")
