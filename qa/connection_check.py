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
    for client in ["codex", "claude", "cursor", "vscode", "cline", "hermes", "openclaw"]:
        path = root / (client + (".toml" if client == "codex" else ".json"))
        original = '# Keep my comment\nmodel = "keep-model"\n[mcp_servers.other]\ncommand = "keep"\n' if client == "codex" else json.dumps({"keep": True, "servers" if client == "vscode" else "mcpServers": {"other": {"command": "keep"}}})
        if client == "hermes":
            original = '# Keep my comment\nmodel: keep-model\nlegacy_flag: on\nmcp_servers:\n  other:\n    command: keep\n# Keep this setting\nagent:\n  max_turns: 12\n'
        elif client == "openclaw":
            original = '// Keep my comment\n{ keep: true, mcp: { servers: { other: { command: "keep", }, }, }, }'
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
        if client in ("hermes", "openclaw"):
            backups = list(path.parent.glob(path.name + ".rhyven-backup-*"))
            assert len(backups) == 1 and backups[0].read_text() == original
            assert backups[0].stat().st_mode & 0o077 == 0
            if client == "hermes":
                assert '# Keep my comment\nmodel: keep-model\nlegacy_flag: on\n' in path.read_text()
                assert 'agent:\n  max_turns: 12\n' in path.read_text()
                entries = json.loads(path.read_text().split('mcp_servers: ', 1)[1].splitlines()[0])
            else:
                entries = json.loads(path.read_text())["mcp"]["servers"]
                assert entries['rhyven']['transport'] == 'stdio'
            assert entries['other']['command'] == 'keep'
            assert entries['rhyven']['command'] == binary
            assert entries['rhyven']['args'][-1] == 'mcp'
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

    # Config path overrides support profiles without touching the user's real settings.
    for client, key, location in [
        ("hermes", "HERMES_HOME", root / "hermes-profile"),
        ("openclaw", "OPENCLAW_STATE_DIR", root / "openclaw-profile"),
        ("openclaw", "OPENCLAW_CONFIG_PATH", root / "custom-claw.json"),
    ]:
        env = dict(os.environ, HOME=str(root / "fake-user"))
        for name in ["HERMES_HOME", "OPENCLAW_STATE_DIR", "OPENCLAW_CONFIG_PATH"]: env.pop(name, None)
        env[key] = str(location)
        result = cli("connect", "--client", client, env=env)
        expected = location if key == "OPENCLAW_CONFIG_PATH" else location / ("config.yaml" if client == "hermes" else "openclaw.json")
        assert result["config_path"] == str(expected) and expected.is_file()
        assert not result["client_session_verified"]
    for client, content in [("openclaw", '{"mcp":false}'), ("openclaw", '{"mcp":{"servers":[]}}'), ("hermes", "mcp_servers: []\n"), ("hermes", '{mcp_servers: {other: {command: keep}}}')]:
        path = root / "bad-settings"
        path.write_text(content)
        assert cli("connect", "--client", client, "--config", str(path), ok=False)["code"] == "configuration"
        assert path.read_text() == content
    for client in ["hermes", "openclaw"]:
        path = root / (client + "-link")
        path.symlink_to(root / "missing")
        assert cli("connect", "--client", client, "--config", str(path), ok=False)["code"] == "configuration"
        assert not (root / "missing").exists()
        assert cli("connect", "--client", client, "--name", "__proto__", "--print", ok=False)["code"] == "configuration"

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
print("PASS: startup instructions; real MCP probe; seven safe config adapters; reruns/conflicts; remote collection guard; bearer auth; no token disclosure")
