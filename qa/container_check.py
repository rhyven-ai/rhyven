"""Real Docker, CLI, three-tool MCP, REST, collections, retries and standalone export.
Usage: python3 qa/container_check.py target/debug/rhyven /path/to/image.id
Build examples/container-python with docker build --iidfile first.
"""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time

from universal_market_check import Client, binary, market

image = Path(sys.argv[2]).read_text().strip()
category = "example/text-analysis"

with tempfile.TemporaryDirectory(prefix="rhyven-container-check-") as temporary:
    home = Path(temporary)

    def cli(*args, collection="global", error=None):
        result = subprocess.run([binary, "--home", str(home), "--collection", collection, *args], capture_output=True, text=True)
        if error:
            assert result.returncode != 0, result.stdout
            value = json.loads(result.stderr)
            assert value["code"] == error, value
            return value
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)

    source = home / "source"
    bundle = home / "app.json"
    cli("app", "init", category, "--dir", str(source), "--runtime", "container")
    cli("app", "package", str(source), "--image", image, "--out", str(bundle))
    cli("app", "test", str(bundle), error="permission_review_required")
    assert cli("app", "test", str(bundle), "--allow-container")["passed"]
    cli("install", str(bundle), error="permission_review_required")
    cli("install", str(bundle), "--accept-permissions")
    cli("install", "rhyven/work-management", "--accept-permissions")
    root = home / "collections/global"
    client = Client(root)
    try:
        manifest = client.tool("rhyven_describe", {"category": category})
        assert manifest["functions"][0]["outputSchema"]["properties"]["words"]["type"] == "integer"
        args = {"text": "hello world", "request_id": "first"}
        first = client.call(category, "action_analyze", args)
        assert first["words"] == 2 and first["calls"] == 1
        assert client.call(category, "action_analyze", args) == first
        client.call(category, "action_analyze", {"text": 123}, error="validation")
        task = client.call("rhyven/work-management", "object_task_create", {"data": {"title": "Mixed execution"}})
        assert task["data"]["title"] == "Mixed execution"
        cli("install", str(bundle), "--accept-permissions", collection="other")
        other = cli("call", "rhyven_call", json.dumps({"category":category,"function":"action_analyze","args":{"text":"private"}}), collection="other")
        assert other["calls"] == 1
        cli("app", "publish", str(bundle))
        request = client.call(market, "action_prepare_remove", {"app": category})
        assert request["execution"]["image"] == image
        client.call(market, "action_apply", {"request_id": request["request_id"]}, consent=True)
        request = client.call(market, "action_prepare_install", {"app": category})
        client.call(market, "action_apply", {"request_id": request["request_id"]}, consent=True)
        assert client.call(category, "action_analyze", {"text":"retained"})["calls"] == 2

        changed = json.loads(bundle.read_text())
        changed["version"] = "0.2.0"
        changed["actions"]["analyze"]["output"]["properties"]["words"]["type"] = "string"
        update = home / "update.json"
        update.write_text(json.dumps(changed))
        cli("update", str(update), "--accept-permissions")
        failed = {"text":"one effect", "request_id":"invalid-output"}
        client.call(category, "action_analyze", failed, error="validation")
        client.call(category, "action_analyze", failed, error="container_incomplete")
        changed["version"] = "0.3.0"
        changed["actions"]["analyze"]["output"]["properties"]["words"]["type"] = "integer"
        update.write_text(json.dumps(changed))
        cli("update", str(update), "--accept-permissions")
        assert client.call(category, "action_analyze", {"text":"after failure"})["calls"] == 4
    finally:
        client.close()

    env = dict(os.environ, RHYVEN_SERVE_TOKEN="container-test-token-0123456789")
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    server = subprocess.Popen([binary,"--home",str(home),"serve","--port",str(port)], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        for _ in range(100):
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=.1):
                    break
            except OSError:
                assert server.poll() is None, server.stderr.read().decode()
                time.sleep(.05)
        client = Client(home / "remote-client", f"http://127.0.0.1:{port}", env)
        try:
            assert client.call(category,"action_analyze",{"text":"via REST"})["calls"] == 5
        finally:
            client.close()
    finally:
        server.terminate()
        server.wait(timeout=5)

    exported = home / "standalone"
    cli("app", "export-mcp", category, "--out", str(exported))
    messages = [
        {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}},
        {"jsonrpc":"2.0","method":"notifications/initialized"},
        {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"action_analyze","arguments":{"args":{"text":"exported"}}}}
    ]
    result = subprocess.run([str(exported / "launch.sh")], input="".join(json.dumps(m)+"\n" for m in messages), text=True, capture_output=True, timeout=30)
    assert result.returncode == 0, result.stderr
    response = json.loads(result.stdout.splitlines()[-1])["result"]
    assert not response["isError"], response
    assert json.loads(response["content"][0]["text"])["calls"] == 1

print("PASS: real Python container; mixed app discovery; CLI/REST/MCP; approvals; collections; retries; output validation; update/removal; standalone export")
