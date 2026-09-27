"""Exercise collection routing through actual MCP and REST processes.
python3 qa/collections_check.py target/debug/rhyven
"""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time

from universal_market_check import Client, binary, market


with tempfile.TemporaryDirectory(prefix="rhyven-collections-") as temporary:
    home = Path(temporary)

    def cli(collection, *args):
        return json.loads(subprocess.check_output([
            binary, "--home", str(home), "--collection", collection, *args
        ]))

    cli("shared", "init")
    cli("personal", "init")
    root = home / "collections" / "shared"
    client = Client(root)
    try:
        assert client.tool("rhyven_categories", {})["collection"] == "shared"
        request = client.call(market, "action_prepare_install", {"app": "rhyven/work-management"})
        assert request["target_collection"] == "shared"
        client.call(market, "action_apply", {"request_id": request["request_id"]}, consent=True)
        client.call("rhyven/work-management", "object_task_create", {"data": {"title": "Shared record"}})
        assert cli("personal", "list") == []
    finally:
        client.close()

    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    env = dict(os.environ, RHYVEN_SERVE_TOKEN="collection-test-token-0123456789")
    server = subprocess.Popen([
        binary, "--home", str(home), "--collection", "shared", "serve", "--port", str(port)
    ], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        for _ in range(100):
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=.1):
                    break
            except OSError:
                if server.poll() is not None:
                    raise AssertionError(server.stderr.read().decode())
                time.sleep(.05)
        client = Client(home / "collections/personal", f"http://127.0.0.1:{port}", env)
        try:
            manifest = client.tool("rhyven_describe", {"category": "rhyven/work-management"})
            assert manifest["scope"]["collection"] == "shared"
            result = client.call("rhyven/work-management", "object_task_query", {})
            assert result["total"] == 1
            assert result["items"][0]["data"]["title"] == "Shared record"
            assert cli("personal", "list") == []
        finally:
            client.close()
    finally:
        server.terminate()
        server.wait(timeout=5)

print("PASS: collection discovery, targeted consent, local state isolation and remote server routing")
