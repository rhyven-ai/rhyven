"""Real persistent Docker service lifecycle and cross-app acceptance.

python3 qa/service_check.py target/debug/rhyven /tmp/rhyven-service-example.id
Uses an isolated home; never publishes images, packages, or Git changes.
"""
import concurrent.futures
import json
import os
from pathlib import Path
import socket
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request
import urllib.error

from universal_market_check import Client

binary = str(Path(sys.argv[1]).resolve())
image = Path(sys.argv[2]).read_text().strip()
app = "example/background-counter"


def eventually(check, seconds=30):
    until = time.monotonic() + seconds
    last = None
    while time.monotonic() < until:
        try:
            last = check()
            if last:
                return last
        except (AssertionError, OSError) as exc:
            last = str(exc)
        time.sleep(.1)
    raise AssertionError(f"Condition timed out: {last}")


with tempfile.TemporaryDirectory(prefix="rv-svc-") as tmp:
    home = Path(tmp)

    def cli(*args, collection="global", error=None):
        p = subprocess.run([binary, "--home", str(home), "--collection", collection,
                            *args], text=True, capture_output=True, timeout=180)
        if error:
            assert p.returncode != 0, p.stdout
            value = json.loads(p.stderr)
            assert value["code"] == error, value
            return value
        assert p.returncode == 0, (args, p.stderr)
        return json.loads(p.stdout)

    def call(action, args=None, **kwargs):
        return cli("call", "rhyven_call", json.dumps({"category": app,
                   "function": "action_" + action, "args": args or {}}), **kwargs)

    def ready(collection="global"):
        return cli("service", "status", app, collection=collection)["state"] == "ready"

    package = json.loads(Path("examples/container-service-python/app.json").read_text())
    package["execution"]["image"] = image
    bundle = home / "app.json"
    bundle.write_text(json.dumps(package))
    daemon_started = False
    server = None
    clients = []
    try:
        cli("app", "validate", str(bundle))
        cli("install", str(bundle), error="permission_review_required")
        cli("install", str(bundle), "--accept-permissions")
        cli("install", "rhyven/project-knowledge", "--accept-permissions")
        call("status", error="service_unavailable")
        daemon = cli("daemon", "start")
        daemon_started = True
        assert cli("daemon", "start")["pid"] == daemon["pid"]
        call("status", error="service_unavailable")  # manual start policy
        with concurrent.futures.ThreadPoolExecutor(2) as pool:
            starts = list(pool.map(lambda _: cli("service", "start", app), range(2)))
        assert starts[0]["generation"] == starts[1]["generation"], starts
        assert cli("service", "list")["services"][0]["app"] == app
        unit = home / "supervisor.service"
        assert cli("daemon", "unit", "--out", str(unit))["enabled"] is False
        assert "daemon" in unit.read_text() and "ExecStart=" in unit.read_text()
        initial = call("status")
        time.sleep(.8)
        assert call("status")["ticks"] > initial["ticks"]
        one = call("increment", {"amount": 3, "request_id": "once"})
        assert one["calls"] == 3
        assert call("increment", {"amount": 3, "request_id": "once"}) == one
        call("increment", {"amount": 4, "request_id": "once"}, error="idempotency_conflict")
        call("increment", {"amount": "bad"}, error="validation")

        client = Client(home / "collections/global")
        clients.append(client)
        assert len(client.request("tools/list", {})["tools"]) == 3
        description = client.tool("rhyven_describe", {"category": app})
        assert description["contract"]["execution"]["mode"] == "service"
        note = client.call(app, "action_remember", {"title": "Shared state", "body": "A background service writes through the declarative app contract.", "request_id": "knowledge-once"})
        record = client.call("rhyven/project-knowledge", "object_note_get", {"id": note["note_id"]})
        assert record["data"]["title"] == "Shared state"
        assert record["updated_by"].startswith("_rhyven_service_")
        assert client.call("rhyven/runtime", "action_service_status", {"app": app})["state"] == "ready"

        # Explicit stop survives daemon restart and an attempted app call.
        cli("service", "stop", app)
        call("status", error="service_unavailable")
        assert call("increment", {"amount": 3, "request_id": "once"}) == one
        cli("daemon", "stop")
        eventually(lambda: not (home / "supervisor/control.sock").exists())
        daemon = cli("daemon", "start")
        call("status", error="service_unavailable")
        cli("service", "start", app)
        assert call("status")["calls"] == 3

        # Enabled intent survives a clean daemon restart.
        os.kill(daemon["pid"], signal.SIGTERM)
        eventually(lambda: not (home / "supervisor/control.sock").exists())
        cli("daemon", "start")
        eventually(ready)
        assert call("status")["calls"] == 3

        # A second collection has independent state and a different process.
        cli("install", str(bundle), "--accept-permissions", collection="other")
        cli("service", "start", app, collection="other")
        assert call("status", collection="other")["calls"] == 0

        # Snapshot all continuous writers, then resume the original instance intent.
        archive = home / "backup.rhyven"
        cli("backup", "global", "--out", str(archive))
        eventually(ready)
        cli("restore", str(archive), "--accept-permissions", collection="restored")
        assert cli("service", "status", app, collection="restored")["state"] == "stopped"
        cli("service", "start", app, collection="restored")
        assert call("status", collection="restored")["calls"] == 3
        cli("service", "stop", app, collection="restored")

        # A staged service health check runs without peer capabilities.
        package["version"] = "0.2.0"
        bundle.write_text(json.dumps(package))
        cli("update", str(bundle), error="permission_review_required")
        assert ready()
        updated = cli("update", str(bundle), "--accept-permissions")
        assert Path(updated["recovery_backup"]).is_file()
        eventually(ready)
        assert call("status")["calls"] == 3

        # REST-backed MCP shares the very same instance and persistent state.
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        env = dict(os.environ, RHYVEN_SERVE_TOKEN="service-test-read-token-123456", RHYVEN_MANAGEMENT_TOKEN="service-test-manage-token-123456")
        server = subprocess.Popen([binary,"--home",str(home),"serve","--port",str(port)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        def listening():
            with socket.create_connection(("127.0.0.1", port), timeout=.1): return True
        eventually(listening)
        read_env = {k:v for k,v in env.items() if k != "RHYVEN_MANAGEMENT_TOKEN"}
        remote = Client(home / "remote", f"http://127.0.0.1:{port}", read_env)
        clients.append(remote)
        assert remote.call(app, "action_increment", {"amount":2})["calls"] == 5
        assert call("status")["calls"] == 5
        remote.call("rhyven/runtime", "action_service_stop", {"app":app}, error="permission")
        # Both route forms, including trailing slashes, enforce management access.
        for route in ["categories/rhyven/runtime/functions/action_service_stop/",
                      "apps/rhyven/runtime/actions/service_stop/"]:
            req = urllib.request.Request(f"http://127.0.0.1:{port}/{route}",
                data=json.dumps({"app":app}).encode(),
                headers={"Authorization":"Bearer " + env["RHYVEN_SERVE_TOKEN"],
                         "Content-Type":"application/json"})
            try:
                urllib.request.urlopen(req)
                raise AssertionError("Unprivileged stop succeeded")
            except urllib.error.HTTPError as exc:
                assert exc.code == 403
        req = urllib.request.Request(f"http://127.0.0.1:{port}/categories",
            headers={"Authorization":"Bearer " + env["RHYVEN_SERVE_TOKEN"],
                     "X-Rhyven-Actor":"_rhyven_service_forged"})
        try:
            urllib.request.urlopen(req)
            raise AssertionError("Forged service principal accepted")
        except urllib.error.HTTPError as exc:
            assert exc.code == 403

        cli("uninstall", app)
        call("status", error="not_installed")
        cli("install", str(bundle), "--accept-permissions")
        call("status", error="service_unavailable")
        cli("service", "start", app)
        assert call("status")["calls"] == 5
        cli("service", "stop", app)
        cli("app", "test", str(bundle), "--allow-container")
        exported = home / "export"
        cli("app", "export-mcp", app, "--out", str(exported))
        try:
            messages = [
                {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}},
                {"jsonrpc":"2.0","method":"notifications/initialized"},
                {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"action_increment","arguments":{"args":{"amount":2}}}}
            ]
            result = subprocess.run([str(exported / "launch.sh")],
                input="".join(json.dumps(m)+"\n" for m in messages),
                text=True,capture_output=True,timeout=60)
            assert result.returncode == 0, result.stderr
            response = json.loads(result.stdout.splitlines()[-1])["result"]
            assert not response["isError"], response
            assert json.loads(response["content"][0]["text"])["calls"] == 2
        finally:
            subprocess.run([str(exported/"rhyven"),"--workspace",str(exported/"state"),
                            "daemon","stop"],capture_output=True,timeout=120)
            eventually(lambda:not (exported/"state/.rhyven/supervisor/control.sock").exists())
        package["execution"]["start_policy"] = "on-demand"
        bundle.write_text(json.dumps(package))
        cli("install", str(bundle), "--accept-permissions", collection="ondemand")
        assert call("status", collection="ondemand")["calls"] == 0
        cli("service", "stop", app, collection="ondemand")
        call("status", collection="ondemand", error="service_unavailable")
        archive = home / "ondemand.rhyven"
        cli("backup", "ondemand", "--out", str(archive))
        cli("restore", str(archive), "--accept-permissions", collection="restored-demand")
        call("status", collection="restored-demand", error="service_unavailable")
        cli("service", "start", app, collection="restored-demand")
        assert call("status", collection="restored-demand")["calls"] == 0
        cli("service", "stop", app, collection="restored-demand")
        package["execution"]["cpus"] = 5
        bundle.write_text(json.dumps(package))
        cli("install", str(bundle), "--accept-permissions", collection="budget")
        cli("service", "start", app, collection="budget", error="service_unavailable")
        cli("service", "stop", app, collection="budget")
        print("PASS: persistent ticks; two-client single instance; scoped declarative callback; retry receipts; manual/on-demand stop; daemon restart and SIGTERM; generated unit; collections; backup/restore; staged update; MCP/REST access control; retained reinstall; app conformance; standalone export; resource admission", flush=True)
    finally:
        for client in clients:
            client.close()
        if server:
            server.terminate()
            server.wait(timeout=5)
        if daemon_started:
            subprocess.run([binary,"--home",str(home),"daemon","stop"], capture_output=True, timeout=120)
            eventually(lambda: not (home / "supervisor/control.sock").exists())
