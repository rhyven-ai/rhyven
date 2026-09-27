"""Real MCP handshake, safe client config merging, startup instructions and REST routing."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request

binary = str(Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory(prefix="rv-connect-") as temporary:
    root = Path(temporary)
    home = root / "home with ' quote"
    prefix = [binary, "--home", str(home), "--collection", "alpha"]

    def cli(*args, ok=True, env=None):
        result = subprocess.run(prefix + list(args), capture_output=True, text=True, timeout=45, env=env)
        assert (result.returncode == 0) == ok, (args, result.stdout, result.stderr)
        return json.loads(result.stdout if ok else result.stderr)

    for args in [(), ("--agent",)]:
        value = cli(*args)
        assert value["status"] == "instructions" and value["collection"] == "alpha"
        assert value["configuration"]["mcpServers"]["rhyven"]["args"][-1] == "mcp"
    probe = cli("connect", "--check")
    assert probe["server_verified"] and not probe["client_session_verified"]
    assert probe["verification"]["server"]["version"] == subprocess.check_output([binary, "--version"], text=True).strip().split()[1]
    assert probe["collection"] == "alpha"
    for client in ["codex", "claude", "cursor", "vscode", "cline"]:
        path = root / (client + (".toml" if client == "codex" else ".json"))
        original = '# Keep my comment\nmodel = "keep-model"\n[mcp_servers.other]\ncommand = "keep"\n' if client == "codex" else json.dumps({"keep": True, "servers" if client == "vscode" else "mcpServers": {"other": {"command": "keep"}}})
        path.write_text(original)
        args = ("connect", "--client", client, "--config", str(path))
        assert cli(*args, "--print")["status"] == "instructions" and path.read_text() == original
        report = cli(*args)
        assert report["configured"] and report["server_verified"]
        assert "keep" in path.read_text() and (client != "codex" or "# Keep my comment" in path.read_text())
        if client == "codex":
            path.write_text(path.read_text().replace('[mcp_servers.rhyven]','[mcp_servers.rhyven]\nenabled = false'))
            assert cli(*args, ok=False)["code"] == "configuration"
            cli(*args, "--replace")
            assert "enabled = false" not in path.read_text()
        installed = path.read_bytes()
        assert not cli(*args)["configuration_changed"] and path.read_bytes() == installed
        other = prefix.copy()
        prefix[-1] = "beta"
        assert cli(*args, ok=False)["code"] == "configuration" and path.read_bytes() == installed
        assert cli(*args, "--replace")["collection"] == "beta"
        prefix[:] = other
        path.write_text("invalid config [")
        assert cli(*args, "--replace", ok=False)["code"] == "configuration"
        assert path.read_text() == "invalid config ["

    secret = "private-connection-token-0123456789"
    env = dict(os.environ, RHYVEN_SERVE_TOKEN=secret)
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    server = subprocess.Popen(prefix + ["serve", "--port", str(port)], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        startup = json.loads(server.stderr.readline())
        assert startup["status"] == "listening" and secret not in json.dumps(startup)
        endpoint = f"http://127.0.0.1:{port}"
        request = urllib.request.Request(endpoint + "/connection", headers={"Authorization": "Bearer " + secret})
        with urllib.request.urlopen(request) as response:
            info = json.load(response)
        assert info["collection"] == "alpha" and info["native_http_mcp"] is False
        try:
            urllib.request.urlopen(endpoint + "/connection")
            raise AssertionError("Connection info bypassed authentication")
        except urllib.error.HTTPError as error:
            assert error.code == 401
        # The local beta collection cannot override the shared alpha collection.
        prefix[-1] = "beta"
        value = cli("connect", "--check", "--server", endpoint, "--expect-collection", "alpha", env=env)
        assert value["collection"] == "alpha" and value["server_verified"]
        path = root / "remote.toml"
        assert cli("connect", "--client", "codex", "--config", str(path), "--server", endpoint, "--expect-collection", "beta", env=env, ok=False)["code"] == "collection"
        assert not path.exists()
        value = cli("connect", "--client", "codex", "--config", str(path), "--server", endpoint, "--expect-collection", "alpha", env=env)
        assert secret not in path.read_text() and "RHYVEN_SERVE_TOKEN" in path.read_text()
        assert secret not in json.dumps(value)
        bad = dict(env, RHYVEN_SERVE_TOKEN="wrong-token-0123456789")
        assert cli("connect", "--check", "--server", endpoint, env=bad, ok=False)["code"] == "connection"
    finally:
        server.terminate()
        server.wait(timeout=10)
print("PASS: startup instructions; real MCP probe; five safe config adapters; reruns/conflicts; remote collection guard; bearer auth; no token disclosure")
