"""Three-tool MCP, real user-elicitation protocol, marketplace lifecycle and REST parity.
Only stdlib; uses temporary workspaces. python3 qa/universal_market_check.py BINARY
"""
import json
import os
from pathlib import Path
import selectors
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
import urllib.error

binary = str(Path(sys.argv[1]).resolve())
market = "rhyven/marketplace"


def cli(root, *args):
    return json.loads(subprocess.check_output([binary, "--workspace", str(root), *args]))


class Client:
    def __init__(self, root, server=None, env=None, elicitation=True, actor="agent", response_timeout=15):
        args = [binary, "--workspace", str(root), "--actor", actor, "mcp"]
        if server:
            args += ["--server", server]
        self.p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.p.stdout, selectors.EVENT_READ)
        self.seq = 0
        self.prompts = []
        self.response_timeout = response_timeout
        self.request("initialize", {"protocolVersion": "2025-11-25", "capabilities": {"elicitation": {"form": {}}} if elicitation else {}, "clientInfo": {"name": "market-test", "version": "1"}})
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        assert [t["name"] for t in self.request("tools/list", {})["tools"]] == ["rhyven_categories", "rhyven_describe", "rhyven_call"]

    def send(self, message):
        self.p.stdin.write(json.dumps(message) + "\n")
        self.p.stdin.flush()

    def request(self, method, params, consent=None):
        self.seq += 1
        seq = self.seq
        self.send({"jsonrpc": "2.0", "id": seq, "method": method, "params": params})
        while True:
            assert self.selector.select(self.response_timeout), "MCP response timed out"
            line = self.p.stdout.readline()
            assert line, self.p.stderr.read()
            message = json.loads(line)
            if message.get("method") == "elicitation/create":
                assert consent is not None, "Unexpected user approval prompt"
                prompt = message["params"]
                assert "stars" in prompt["message"] and "permissions" in prompt["message"] and "target_workspace" in prompt["message"]
                self.prompts.append(prompt)
                self.send({"jsonrpc": "2.0", "id": message["id"], "result": {"action": "accept" if consent else "decline", "content": {"approve": consent}}})
                continue
            assert message["id"] == seq and "error" not in message, message
            return message["result"]

    def tool(self, name, arguments, consent=None, error=None):
        response = self.request("tools/call", {"name": name, "arguments": arguments}, consent)
        value = json.loads(response["content"][0]["text"])
        if error:
            assert response["isError"] and value["code"] == error, value
        else:
            assert not response["isError"], value
        return value

    def call(self, category, function, args, **kwargs):
        return self.tool("rhyven_call", {"category": category, "function": function, "args": args}, **kwargs)

    def close(self):
        self.p.stdin.close()
        try:
            assert self.p.wait(timeout=5) == 0, self.p.stderr.read()
        finally:
            if self.p.poll() is None:
                self.p.kill()
                self.p.wait()
            self.selector.close()


def workflow(root, server=None, env=None):
    c = Client(root, server, env)
    try:
        assert any(a["name"] == market for a in c.tool("rhyven_categories", {})["apps"])
        manifest = c.tool("rhyven_describe", {"category": market})
        assert manifest["guidance_markdown"]
        assert not any(f["name"].endswith("_create") for f in manifest["functions"])
        items = c.call(market, "object_listing_query", {"filters": {"search": "inventory"}})
        assert items["total"] == 1
        assert items["items"][0]["data"]["stars_status"] == "unavailable"
        c.call(market, "object_listing_get", {"id": "rhyven/inventory"})
        c.call(market, "not_a_function", {}, error="not_found")
        req = c.call(market, "action_prepare_install", {"app": "rhyven/inventory"})
        c.call(market, "action_apply", {"request_id": req["request_id"], "approved": True}, error="validation")
        c.call(market, "action_apply", {"request_id": req["request_id"]}, consent=False, error="approval_required")
        assert not any(a["name"] == "rhyven/inventory" for a in c.tool("rhyven_categories", {})["apps"])
        req = c.call(market, "action_prepare_install", {"app": "rhyven/inventory"})
        c.call(market, "action_apply", {"request_id": req["request_id"]}, consent=True)
        before = len(c.prompts)
        c.call(market, "action_apply", {"request_id": req["request_id"]})
        assert len(c.prompts) == before
        c.tool("rhyven_describe", {"category": "rhyven/inventory"})
        c.call("rhyven/inventory", "object_asset_create", {"data": {"label": 7, "serial": "test"}}, error="validation")
        args = {"data": {"label": "Keep this record", "serial": "test"}, "request_id": "one"}
        record = c.call("rhyven/inventory", "object_asset_create", args)
        assert c.call("rhyven/inventory", "object_asset_create", args) == record
        record = c.call("rhyven/inventory", "action_retire", {"id": record["id"], "expected_revision": 1, "request_id": "retire-once"})
        assert record["data"]["status"] == "retired"
        c.call("rhyven/inventory", "object_asset_update", {"id": record["id"], "patch": {"label": "stale"}, "expected_revision": 1}, error="revision_conflict")
        if server is None:
            package = json.loads((Path(__file__).resolve().parents[1] / "catalog/inventory.json").read_text())
            package["version"] = "0.4.0"
            package["objects"]["asset"]["schema"]["properties"]["color"] = {"type": "string"}
            upgrade = Path(root) / "upgrade.json"
            upgrade.write_text(json.dumps(package))
            cli(root, "app", "publish", str(upgrade))
            listing = c.call(market, "object_listing_query", {"filters": {"update_available": True}})
            assert listing["total"] == 1
            req = c.call(market, "action_prepare_update", {"app": "rhyven/inventory"})
            c.call(market, "action_apply", {"request_id": req["request_id"]}, consent=True)
            assert c.tool("rhyven_describe", {"category": "rhyven/inventory"})["version"] == "0.4.0"
            assert c.call("rhyven/inventory", "object_asset_get", {"id": record["id"]}) == record
        req = c.call(market, "action_prepare_remove", {"app": "rhyven/inventory"})
        assert c.call(market, "action_apply", {"request_id": req["request_id"]}, consent=True)["data_retained"]
        assert not any(a["name"] == "rhyven/inventory" for a in c.tool("rhyven_categories", {})["apps"])
        req = c.call(market, "action_prepare_install", {"app": "rhyven/inventory"})
        c.call(market, "action_apply", {"request_id": req["request_id"]}, consent=True)
        assert c.call("rhyven/inventory", "object_asset_get", {"id": record["id"]}) == record
    finally:
        c.close()


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="rhyven-universal-") as temp:
        root = Path(temp)
        workflow(root / "local")
        c = Client(root / "no-elicitation", elicitation=False)
        try:
            req = c.call(market, "action_prepare_install", {"app": "rhyven/inventory"})
            c.call(market, "action_apply", {"request_id": req["request_id"]}, error="approval_required")
            assert not c.prompts
        finally:
            c.close()
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        token = "test-runtime-token-0123456789"
        env = dict(os.environ, RHYVEN_SERVE_TOKEN=token, RHYVEN_MANAGEMENT_TOKEN="test-management-token-0123456789")
        server_root = root / "server"
        server = subprocess.Popen([binary, "--workspace", str(server_root), "serve", "--port", str(port)], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        url = f"http://127.0.0.1:{port}"
        try:
            for _ in range(100):
                try:
                    with socket.create_connection(("127.0.0.1", port), timeout=.1):
                        break
                except OSError:
                    time.sleep(.05)
            no_management = dict(env)
            no_management.pop("RHYVEN_MANAGEMENT_TOKEN")
            c = Client(root / "unprivileged", url, no_management)
            try:
                c.call(market, "object_listing_query", {})
                c.call(market, "action_prepare_install", {"app": "rhyven/inventory"}, error="permission")
            finally:
                c.close()
            for path in ["/categories/rhyven/marketplace/functions/action_prepare_install", "/apps/rhyven/marketplace/actions/prepare_install", "//apps/rhyven/marketplace/actions/prepare_install"]:
                request = urllib.request.Request(url + path, data=b'{"app":"rhyven/inventory","args":{"app":"rhyven/inventory"}}', headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"}, method="POST")
                try:
                    urllib.request.urlopen(request)
                    raise AssertionError("Unprivileged management succeeded")
                except urllib.error.HTTPError as e:
                    assert e.code == 403, e.code
            workflow(root / "remote-client", url, env)
            assert cli(server_root, "list")[0]["name"] == "rhyven/inventory"
            assert not (root / "remote-client" / ".rhyven" / "state.sqlite3").exists()
        finally:
            server.terminate()
            server.wait(timeout=5)
    print("PASS: three tools, manifests, consent accept/decline, no-host fallback, live discovery, idempotency, schema/revision checks, removal/data retention, REST parity and management authorization")
