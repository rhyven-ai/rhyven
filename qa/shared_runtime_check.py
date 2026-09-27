"""Exercise the authenticated shared runtime through REST and REST-backed MCP.

Usage: python3 qa/shared_runtime_check.py /absolute/path/to/rhyven
Uses only Python's standard library and a temporary local workspace.
"""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

binary = str(Path(sys.argv[1]).resolve())
token = "shared-runtime-test-token-0123456789"


def command(root, *args, env=None):
    return json.loads(subprocess.check_output([binary, "--workspace", str(root), *args], env=env))


def api(port, method, path, body=None, actor="http-client", auth=token):
    data = None if body is None else json.dumps(body).encode()
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}{path}", data=data, method=method,
        headers={"Authorization": f"Bearer {auth}", "Content-Type": "application/json", "X-Rhyven-Actor": actor},
    )
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return response.status, json.loads(response.read())
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read())


def wait(port):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                return
        except OSError:
            time.sleep(0.05)
    raise AssertionError("shared server did not start")


def universal_arguments(name, arguments):
    args = dict(arguments)
    if name == "list_apps":
        return "rhyven_categories", {}
    if name == "describe_app":
        return "rhyven_describe", {"category": args["app"]}
    category = args.pop("app")
    if name == "execute":
        function = "action_" + args.pop("action")
        data = args.pop("args")
        data.update(args)
    else:
        function = "object_" + args.pop("object") + "_" + name
        data = args
    return "rhyven_call", {"category": category, "function": function, "args": data}


def remote_mcp(root, port, env):
    process = subprocess.Popen(
        [binary, "--workspace", str(root), "mcp", "--server", f"http://127.0.0.1:{port}"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env,
    )
    sequence = 0

    def request(method, params, notification=False):
        nonlocal sequence
        sequence += 1
        message = {"jsonrpc": "2.0", "method": method, "params": params}
        if not notification:
            message["id"] = sequence
        process.stdin.write(json.dumps(message) + "\n")
        process.stdin.flush()
        if notification:
            return None
        row = json.loads(process.stdout.readline())
        assert row["id"] == sequence and "error" not in row, row
        return row["result"]

    def call(name, arguments):
        name, arguments = universal_arguments(name, arguments)
        result = request("tools/call", {"name": name, "arguments": arguments})
        assert not result["isError"], result
        return json.loads(result["content"][0]["text"])

    try:
        request("initialize", {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "shared-check", "version": "1"}})
        request("notifications/initialized", {}, True)
        assert len(request("tools/list", {})["tools"]) == 3
        app = "rhyven/work-management"
        assert any(item["name"] == app for item in call("list_apps", {})["apps"])
        task = call("create", {"app": app, "object": "task", "data": {"title": "Created over REST-backed MCP"}, "request_id": "remote-mcp-create"})
        assigned = call("execute", {"app": app, "action": "assign", "args": {"id": task["id"], "expected_revision": 1, "owner": "remote-agent"}})
        assert assigned["revision"] == 2 and assigned["data"]["status"] == "in_progress"
        return assigned
    finally:
        process.stdin.close()
        assert process.wait(timeout=5) == 0, process.stderr.read()


with tempfile.TemporaryDirectory(prefix="rhyven-shared-") as temp:
    root = Path(temp) / "runtime"
    port_socket = socket.socket()
    port_socket.bind(("127.0.0.1", 0))
    port = port_socket.getsockname()[1]
    port_socket.close()
    command(root, "init")
    command(root, "install", "rhyven/work-management", "--accept-permissions")
    environment = dict(os.environ, RHYVEN_SERVE_TOKEN=token)
    server = subprocess.Popen([binary, "--workspace", str(root), "serve", "--port", str(port)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=environment)
    try:
        wait(port)
        assert api(port, "GET", "/apps", auth="wrong")[0] == 401
        status, apps = api(port, "GET", "/apps")
        assert status == 200 and [app["name"] for app in apps] == ["rhyven/work-management", "rhyven/marketplace", "rhyven/runtime"]
        status, task = api(port, "POST", "/apps/rhyven/work-management/objects/task", {"data": {"title": "Created by direct HTTP"}, "request_id": "http-create"}, actor="client-a")
        assert status == 200 and task["revision"] == 1
        status, retry = api(port, "POST", "/apps/rhyven/work-management/objects/task", {"data": {"title": "Created by direct HTTP"}, "request_id": "http-create"}, actor="client-a")
        assert status == 200 and retry == task
        status, conflict = api(port, "PATCH", f"/apps/rhyven/work-management/objects/task/{task['id']}", {"patch": {"title": "stale"}, "expected_revision": 2}, actor="client-b")
        assert status == 409 and conflict["code"] == "revision_conflict"
        status, fetched = api(port, "GET", f"/apps/rhyven/work-management/objects/task/{task['id']}", actor="client-b")
        assert status == 200 and fetched == task
        remote = remote_mcp(root, port, environment)
        status, queried = api(port, "POST", "/apps/rhyven/work-management/query", {"object": "task", "filters": {"status": "in_progress"}}, actor="client-b")
        assert status == 200 and any(item["id"] == remote["id"] for item in queried["items"])
        assert command(root, "call", "get", json.dumps({"app": remote["app"], "object": "task", "id": remote["id"]}))["revision"] == 2
    finally:
        server.terminate()
        try:
            server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait()

print("PASS: authenticated REST, two clients, idempotency, revision conflict, REST-backed MCP, and shared SQLite state")
