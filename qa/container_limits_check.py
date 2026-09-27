"""Real container boundary checks. Pass BINARY and probe image ID file."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1]).resolve())
image = Path(sys.argv[2]).read_text().strip()
with tempfile.TemporaryDirectory(prefix="rhyven-limits-") as temporary:
    home = Path(temporary)
    env = dict(os.environ, RHYVEN_SECRET_TEST="test-only", RHYVEN_SECRET_UNDECLARED="test-only")
    def cli(*args, error=None):
        result = subprocess.run([binary,"--home",str(home),*args], env=env, text=True, capture_output=True, timeout=30)
        if error:
            assert result.returncode != 0, result.stdout
            assert json.loads(result.stderr)["code"] == error, result.stderr
        else:
            assert result.returncode == 0, result.stderr
            return json.loads(result.stdout)
    schema = {"type":"object","properties":{},"required":[],"additionalProperties":False}
    output = dict(schema, properties={key:{"type":"boolean"} for key in ["root_readonly","data_readonly","network_denied","declared_secret","undeclared_secret"]})
    p = {"format":2,"name":"test/probe","publisher":"test","version":"0.1.0","description":"Boundary test", "hosting":{"mode":"local"},"execution":{"driver":"container","protocol":"rhyven.container/1","image":image,"timeout_seconds":3},"permissions":["state.read","container.execute"],"objects":{},"actions":{"probe":{"description":"Probe isolation", "input":dict(schema, properties={"mode":{"type":"string"}},required=["mode"]),"output":output}},"guide":"Test fixture", "tests":[]}
    file = home / "app.json"
    def install(update=False):
        file.write_text(json.dumps(p))
        cli("update" if update else "install",str(file),"--accept-permissions")
    def call(mode, error=None):
        return cli("call","rhyven_call",json.dumps({"category":"test/probe","function":"action_probe","args":{"mode":mode,"request_id":mode + p["version"]}}),error=error)
    install()
    result = call("report")
    assert result == {"root_readonly":True,"data_readonly":True,"network_denied":True,"declared_secret":False,"undeclared_secret":False}, result
    p["version"] = "0.2.0"
    p["permissions"] += ["state.write","secrets.read"]
    p["execution"]["secrets"] = ["RHYVEN_SECRET_TEST"]
    install(True)
    result = call("report")
    assert result["declared_secret"] and not result["undeclared_secret"] and not result["data_readonly"] and result["root_readonly"] and result["network_denied"], result
    call("invalid", error="container_protocol")
    call("overflow", error="container_protocol")
    call("timeout", error="container_timeout")
    call("timeout", error="container_incomplete")
    remaining = subprocess.check_output(["docker","ps","-a","--filter","name=rhyven-","--format","{{.Names}}"], text=True)
    assert not remaining.strip(), remaining
print("PASS: no network by default, read-only root/state, scoped secrets, malformed/oversized output rejection, timeout cleanup and no blind retry")
