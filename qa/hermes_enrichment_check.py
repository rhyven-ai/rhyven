"""Exercise a real Hermes chat, exported hook and MCP confirmation against a loopback model.

Usage: python3 qa/hermes_enrichment_check.py /path/to/rhyven /path/to/hermes
The temporary profile contains one test hook and one local MCP server.
"""
import json
import os
from pathlib import Path
import shlex
import signal
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from uuid import uuid4

BINARY, HERMES = (str(Path(arg).resolve()) for arg in sys.argv[1:])
ENTRY = "quay-trial"
ALIAS = "mcp__quay_trial__rhyven_call"
QUESTION = "Which amber ferry pennant marks the quay handoff?"


class LocalModel(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, data, content_type="application/json"):
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        self.reply(b'{"object":"list","data":[{"id":"local-model","object":"model"}]}')

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if not self.path.endswith("/chat/completions"):
            self.reply(b"{}")
            return
        messages = body["messages"]
        user = next(m["content"] for m in reversed(messages) if m["role"] == "user")
        self.server.requests.append(user)
        message = {"role": "assistant", "content": "Verified."}
        finish = "stop"
        instruction = f"For this connection test, call {ALIAS} with "
        if instruction in user and not any(m["role"] == "tool" for m in messages):
            assert any(t["function"]["name"] == ALIAS for t in body["tools"])
            args, _ = json.JSONDecoder().raw_decode(user.split(instruction, 1)[1])
            function = {"name": ALIAS, "arguments": json.dumps(args)}
            message = {"role": "assistant", "content": None, "tool_calls": [
                {"id": "call_fixture", "type": "function", "function": function}
            ]}
            finish = "tool_calls"
        base = {"id": "chatcmpl-fixture", "created": 1, "model": "local-model"}
        if body.get("stream"):
            if message.get("tool_calls"):
                message["tool_calls"][0]["index"] = 0
            chunks = [dict(base, object="chat.completion.chunk", choices=[
                {"index": 0, "delta": delta, "finish_reason": reason}
            ]) for delta, reason in ((message, None), ({}, finish))]
            stream = "".join("data: " + json.dumps(chunk) + "\n\n" for chunk in chunks)
            self.reply((stream + "data: [DONE]\n\n").encode(), "text/event-stream")
        else:
            self.reply(json.dumps(dict(base, object="chat.completion", choices=[
                {"index": 0, "message": message, "finish_reason": finish}
            ])).encode())


def test(root, server):
    profile = root / "hermes"
    profile.mkdir()
    env = dict(HOME=str(root), HERMES_HOME=str(profile), RHYVEN_HOME=str(root / "state"),
               PATH=os.pathsep.join([str(Path(HERMES).parent), "/usr/local/bin", "/usr/bin", "/bin"]),
               OPENAI_API_KEY="local-fixture", NO_PROXY="localhost,127.0.0.1",
               HTTPS_PROXY="http://127.0.0.1:9", HTTP_PROXY="http://127.0.0.1:9",
               HERMES_ACCEPT_HOOKS="1", PYTHONDONTWRITEBYTECODE="1", TERM="dumb")

    def cli(*args, payload=None):
        command = [BINARY, "--collection", "quay", "--actor", "hermes-worker", *map(str, args)]
        proc = subprocess.run(command, cwd=root, env=env, text=True, capture_output=True,
                              input=json.dumps(payload), timeout=20, check=True)
        return json.loads(proc.stdout)

    adapter = root / "adapter.py"
    cli("context", "export-adapter", adapter)
    command = shlex.join([sys.executable, str(adapter), "hermes", "--binary", BINARY,
                          "--home", str(root / "state"), "--collection", "quay",
                          "--connection", ENTRY, "--actor", "hermes-worker"])
    config = profile / "config.yaml"
    config.write_text('tools:\n  tool_search:\n    enabled: "off"\nhooks:\n'
                      '  pre_llm_call:\n    - command: ' + json.dumps(command) + "\n")
    connection = ["connect", "--client", "hermes", "--name", ENTRY, "--config", str(config)]
    cli(*connection)
    check = [*connection, "--check", "--configured"]
    marker = "PIER_" + uuid4().hex
    cli("call", "rhyven_call", "-", payload={"category": "rhyven/wobbegong",
        "function": "action_ingest_files", "args": {"source": "pilot", "documents": [
            {"id": "quay.md", "text": "The amber ferry pennant for the quay handoff is " + marker}
        ]}})
    cli("context", "enable", ENTRY)
    challenge = cli(*check, "--test-prompt", QUESTION)["diagnostics"]["enrichment"]["submit_in_harness"]

    for case, prompt in (("match", challenge), ("disabled", QUESTION), ("miss", "How is a violin tuned?")):
        if case == "disabled":
            cli("context", "disable", ENTRY)
        if case == "miss":
            cli("context", "enable", ENTRY)
        server.requests = []
        proc = subprocess.run([HERMES, "chat", "-Q", "-t", "all", "--provider", "custom",
                               "--model", "local-model", "--api-key", "local-fixture", "--base-url",
                               f"http://127.0.0.1:{server.server_port}/v1", "-q", prompt],
                              env=env, cwd=root, capture_output=True, text=True, timeout=90)
        assert proc.returncode == 0, proc.stderr[-1000:]
        assert server.requests, case
        if case == "match":
            first = server.requests[0]
            assert first.count(marker) == 1 and f"For more evidence, call {ALIAS}" in first
            assert "Source quay.md" in first
            live = cli(*check)["diagnostics"]["enrichment"]["live_test"]
            assert (live["status"], live["preparation"], live["tool"]) == ("confirmed", "keyword_match", ALIAS)
        else:
            assert all(user == prompt for user in server.requests), case


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="rh-") as directory, ThreadingHTTPServer(("127.0.0.1", 0), LocalModel) as model:
        root = Path(directory)
        worker = threading.Thread(target=model.serve_forever, daemon=True)
        worker.start()
        try:
            test(root, model)
        finally:
            model.shutdown()
            worker.join()
            # An upstream CLI may have started an unmanaged gateway for this profile.
            pid_file = root / "hermes/gateway.pid"
            if pid_file.is_file():
                record = json.loads(pid_file.read_text())
                if record.get("hermes_home") == str(root / "hermes"):
                    pid = int(record["pid"])
                    environ = Path(f"/proc/{pid}/environ")
                    try:
                        owned = os.fsencode(f"HERMES_HOME={root}/hermes") in environ.read_bytes().split(b"\0")
                    except FileNotFoundError:
                        owned = False
                    if owned:
                        os.kill(pid, signal.SIGTERM)
                        for _ in range(100):
                            if not pid_file.exists():
                                break
                            time.sleep(.1)
    print("Real Hermes model turn passed: cited match, MCP confirmation, disabled turn and unrelated turn.")
