"""Deterministic host-preflight regression using a fake Docker CLI; no real isolation claim."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory(prefix="rhyven-host-check-") as tmp:
    root = Path(tmp)
    mock = root / "bin"
    mock.mkdir()
    config = root / "engine.json"
    calls = root / "calls.jsonl"
    docker = mock / "docker"
    docker.write_text('''#!/usr/bin/env python3
import json,os,sys
from pathlib import Path
args=sys.argv[1:]
c=json.loads(Path(os.environ["TEST_ENGINE"]).read_text())
with open(os.environ["TEST_CALLS"],"a") as f: f.write(json.dumps(args)+"\\n")
if args[0]=="context": print("unix:///test/docker.sock")
elif args[0]=="info": print(json.dumps(c))
elif args[:2]==["image","inspect"]: print(json.dumps([{"Os":"linux","Architecture":c.get("image_arch","amd64"),"Config":{}}]))
elif args[0]=="run": print(json.dumps({"result":{"words":1,"sha256":"fixture","calls":1}}))
elif args[0]=="rm": pass
else: sys.exit(2)
''')
    docker.chmod(0o755)
    env = dict(os.environ, PATH=str(mock)+os.pathsep+os.environ["PATH"], TEST_ENGINE=str(config), TEST_CALLS=str(calls))
    env.pop("DOCKER_CONTEXT", None)
    env["DOCKER_HOST"] = "unix:///test/docker.sock"
    healthy = dict(OSType="linux", Architecture="x86_64", CpuCfsQuota=True, CpuCfsPeriod=True, MemoryLimit=True, PidsLimit=True, SecurityOptions=["name=rootless"], CgroupDriver="systemd", CgroupVersion="2")
    def state(**changes):
        config.write_text(json.dumps(dict(healthy, **changes)))
    def cli(*args, error=None):
        p = subprocess.run([binary,"--home",str(root/"home"),*args],env=env,text=True,capture_output=True,timeout=30)
        result = json.loads(p.stderr if p.returncode else p.stdout)
        if error:
            assert p.returncode and result["code"] == error, (p.returncode,result)
        else:
            assert p.returncode == 0, result
        return result
    package = json.loads(Path("examples/container-python/app.json").read_text())
    package["execution"]["image"] = "sha256:" + "a"*64
    app = root/"app.json"
    app.write_text(json.dumps(package))
    state(CpuCfsQuota=False)
    assert not cli("doctor")["container"]["ready"]
    cli("install",str(app),"--accept-permissions",error="container_unavailable")
    assert not any(json.loads(line)[0] in ("pull","image","run") for line in calls.read_text().splitlines())
    cli("install","rhyven/work-management","--accept-permissions")
    state()
    cli("install",str(app),"--accept-permissions")
    args=json.dumps({"category":package["name"],"function":"action_analyze","args":{"text":"test","request_id":"retry-after-preflight"}})
    for failure in [dict(CpuCfsQuota=False),dict(MemoryLimit=False),dict(PidsLimit=False),dict(image_arch="arm64")]:
        state(**failure)
        e=cli("call","rhyven_call",args,error="container_unavailable")
        assert e["kind"] == "UNAVAILABLE"
    assert not any(json.loads(line)[0] == "run" for line in calls.read_text().splitlines())
    state()
    result=cli("call","rhyven_call",args)
    assert result["words"] == 1
    state(CpuCfsQuota=False)
    assert cli("call","rhyven_call",args) == result  # completed receipts need no engine
    report=cli("call","rhyven_call",json.dumps({"category":"rhyven/marketplace","function":"action_doctor","args":{}}))
    assert report["declarative"]["ready"] and not report["container"]["ready"]
    env["DOCKER_HOST"]="tcp://remote:2375"
    assert not cli("doctor")["container"]["ready"]
print("PASS: host preflight, architecture mismatch, retry without poisoned receipt, cached results, agent diagnosis and declarative availability")
