"""Fault injection for real service containers. Supply a built fault-fixture image ID."""
import concurrent.futures
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time

binary = str(Path(sys.argv[1]).resolve())
image = Path(sys.argv[2]).read_text().strip()
app = "example/background-counter"


def eventually(fn, timeout=30):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            if fn(): return
        except (AssertionError, OSError):
            pass
        time.sleep(.1)
    raise AssertionError("Condition did not become true: " + json.dumps(cli("service", "status", app)))


with tempfile.TemporaryDirectory(prefix="rv-fault-") as tmp:
    home = Path(tmp)
    def cli(*args, error=None):
        r = subprocess.run([binary,"--home",str(home),*args], text=True,capture_output=True,timeout=150)
        if error:
            assert r.returncode, r.stdout
            value = json.loads(r.stderr)
            assert value["code"] in ([error] if isinstance(error,str) else error), value
            return value
        assert not r.returncode, (args,r.stderr)
        return json.loads(r.stdout)
    def call(action,args=None,**kw):
        return cli("call","rhyven_call",json.dumps({"category":app,"function":"action_"+action,"args":args or {}}),**kw)
    def ready():
        return cli("service","status",app)["state"] == "ready"
    p=json.loads(Path("examples/container-service-python/app.json").read_text())
    p["execution"].update(image=image,timeout_seconds=2,restart_limit=1)
    empty={"type":"object","properties":{},"additionalProperties":False}
    for action in ["crash","hang","flood","log_flood","fail"]:
        p["actions"][action]={"description":"Test-only fault", "input":empty,"output":empty}
    p["actions"]["bad"]={"description":"Invalid result", "input":empty,"output":empty}
    p["actions"]["delayed"]={"description":"Callback during maintenance", "input":empty,"output":p["actions"]["remember"]["output"]}
    p["actions"]["probe"]={"description":"Probe declared grants", "input":{"type":"object","properties":{"category":{"type":"string"},"function":{"type":"string"},"args":empty},"required":["category","function","args"],"additionalProperties":False},"output":{"type":"object","properties":{"allowed":{"type":"boolean"},"error":{"type":"string"}},"required":["allowed"],"additionalProperties":False}}
    original_version = p["version"]
    bundle=home/"app.json"
    bundle.write_text(json.dumps(p))
    try:
        cli("install",str(bundle),"--accept-permissions")
        cli("install","rhyven/project-knowledge","--accept-permissions")
        daemon=cli("daemon","start")
        cli("service","start",app)
        # No platform management, other functions, or forged actor/collection fields.
        for category,function in [("rhyven/marketplace","action_prepare_install"),("rhyven/runtime","action_service_stop"),("rhyven/project-knowledge","object_note_create")]:
            result=call("probe",{"category":category,"function":function,"args":{}})
            assert result["allowed"] is False and "permission" in result["error"],result
        call("bad",{"request_id":"bad-result"},error="validation")
        call("bad",{"request_id":"bad-result"},error="service_incomplete")
        call("log_flood")
        logs=cli("service","logs",app)
        assert 0 < len(logs["text"].encode()) <= 32768
        before=cli("service","status",app)["generation"]
        call("crash",{"request_id":"crashed"},error=["service_unavailable","service_protocol"])
        eventually(lambda: ready() and cli("service","status",app)["generation"] != before)
        call("crash",{"request_id":"crashed"},error="service_incomplete")
        call("hang",{"request_id":"hung"},error="service_timeout")
        eventually(lambda: cli("service","status",app)["state"] == "failed")
        call("hang",{"request_id":"hung"},error="service_incomplete")
        cli("daemon","stop")
        eventually(lambda:not (home/"supervisor/control.sock").exists())
        daemon=cli("daemon","start")
        time.sleep(1.5)
        assert cli("service","status",app)["state"] == "failed", "Daemon restart must not reset the app crash budget"
        cli("service","start",app)
        before=cli("service","status",app)["generation"]
        call("flood",{"request_id":"flood"},error=["service_protocol","invalid_json"])
        eventually(lambda: ready() and cli("service","status",app)["generation"] != before)
        call("flood",{"request_id":"flood"},error="service_incomplete")

        # Backup during a pending callback is bounded and restores no active process.
        for iteration in range(6):
            with concurrent.futures.ThreadPoolExecutor(2) as pool:
                pending=pool.submit(lambda: call("delayed",{"request_id":f"delayed-{iteration}"},error=["service_unavailable","service_protocol","service_incomplete","app_error"]))
                time.sleep(.1)
                archive=home/f"race-{iteration}.rhyven"
                cli("backup","global","--out",str(archive))
                pending.result(timeout=20)
            eventually(ready)

        # Failed staged validation preserves the old package and live data.
        old=call("status")
        p["version"]="0.2.0"; p["health_action"]="fail"
        bundle.write_text(json.dumps(p))
        cli("update",str(bundle),"--accept-permissions",error="app_error")
        eventually(ready)
        assert cli("service","status",app)["version"] == original_version
        assert call("status")["calls"] == old["calls"]

        # Kill the updater during a staged health check. Live package/data must survive.
        p["version"]="0.2.1"; p["health_action"]="hang"
        p["execution"]["timeout_seconds"]=30
        bundle.write_text(json.dumps(p))
        updater=subprocess.Popen([binary,"--home",str(home),"update",str(bundle),"--accept-permissions"],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        stage=home/"collections/global/update-stage"
        try:
            until=time.monotonic()+20
            while time.monotonic()<until:
                if list(stage.glob("**/control.sock")): break
                assert updater.poll() is None,updater.communicate()
                time.sleep(.05)
            else: raise AssertionError("Staged supervisor did not start")
            updater.kill(); updater.wait(timeout=10)
            cli("service","start",app)
            eventually(ready)
            assert cli("service","status",app)["version"] == original_version
            assert call("status")["calls"] == old["calls"]
            assert not stage.exists(), "Interrupted staging state was not recovered"
        finally:
            if updater.poll() is None: updater.kill(); updater.wait(timeout=10)
            # The isolated supervisor was a thread in the killed updater. Restart it
            # to fence/remove its orphan container before deleting the test home.
            if (stage/"supervisor/control.sock").exists():
                subprocess.run([binary,"--home",str(stage),"daemon","start"],capture_output=True,timeout=120)
                subprocess.run([binary,"--home",str(stage),"daemon","stop"],capture_output=True,timeout=120)

        # An owner crash is recoverable; a new owner fences/removes the old container.
        before=cli("service","status",app)["generation"]
        os.kill(daemon["pid"],signal.SIGKILL)
        eventually(lambda: cli("service","status",app)["supervisor_available"] is False)
        cli("daemon","start")
        eventually(lambda: ready() and cli("service","status",app)["generation"] != before)
        print("PASS: callback denials; invalid-result receipts; bounded logs/frames; crash/restart budget; timeout; backup callback race; failed-update rollback; killed updater recovery; supervisor kill/recovery",flush=True)
    finally:
        subprocess.run([binary,"--home",str(home),"daemon","stop"],capture_output=True,timeout=120)
        eventually(lambda:not (home/"supervisor/control.sock").exists())
