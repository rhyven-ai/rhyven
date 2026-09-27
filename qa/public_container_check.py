"""Install published container apps anonymously on a fresh Docker host.

python3 qa/public_container_check.py BINARY
Requires Docker. Uses temporary state, empty Docker credentials and simulated
MCP host consent. Does not build images or install from local app manifests.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from universal_market_check import Client, market


binary = str(Path(sys.argv[1]).resolve())
documentation = "rhyven/repo-documentation-tool"
messaging = "rhyven/messaging"
for key in ("GH_TOKEN", "GITHUB_TOKEN", "DOCKER_AUTH_CONFIG"):
    os.environ.pop(key, None)

with tempfile.TemporaryDirectory(prefix="rv-public-containers-") as folder:
    home = Path(folder)
    docker_config = home / "docker-config"
    docker_config.mkdir()
    os.environ["DOCKER_CONFIG"] = str(docker_config)

    def cli(*args):
        result = subprocess.run([binary, "--home", folder, *args], capture_output=True, text=True, timeout=420)
        assert result.returncode == 0, (args, result.stdout, result.stderr)
        return json.loads(result.stdout)

    cli("registry-refresh", "rhyven-ai/registry", "--anonymous", "--git-ref", os.environ.get("RHYVEN_TEST_REGISTRY_REF", "main"))
    root = home / "collections/global"
    client = Client(root, response_timeout=420)
    other = None
    daemon = False
    try:
        for category in (documentation, messaging):
            listing = client.call(market, "object_listing_get", {"id": category})["data"]
            assert listing["repository"] == "rhyven-ai/registry", listing
            assert listing["publisher_label"] == "Rhyven", listing
            request = client.call(market, "action_prepare_install", {"app": category})
            image = request["execution"]["image"]
            assert image.startswith("ghcr.io/rhyven-ai/") and "@sha256:" in image
            # A fresh runner must pull the real published image after consent.
            assert subprocess.run(["docker", "image", "inspect", image], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode != 0
            client.call(market, "action_apply", {"request_id": request["request_id"]}, consent=False, error="approval_required")
            assert subprocess.run(["docker", "image", "inspect", image], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode != 0
            request = client.call(market, "action_prepare_install", {"app": category})
            client.call(market, "action_apply", {"request_id": request["request_id"]}, consent=True)
            contract = client.tool("rhyven_describe", {"category": category})["contract"]
            assert contract["execution"]["image"] == image
            print("PASS anonymous package and image install:", category, flush=True)

        def doc(action, **args):
            return client.call(documentation, "action_" + action, {"repository": "public-fixture", **args})

        doc("put_files", files=[{"path": "main.py", "content": "def increment(value):\n    return value + 1\n"}])
        scan = doc("scan")
        assert scan["complete"] and scan["symbols"] == 1, scan
        structure = json.loads(doc("read_artifact", name="structur.json")["content"])
        assert structure["symbols"][0]["start_line"] == 1
        summaries = {
            "symbol": "increment returns its argument plus one without modifying state.",
            "file": "main.py defines the increment function.",
            "directory": "The repository root contains main.py and its increment operation.",
            "subsystem": "This repository provides a stateless increment operation.",
            "main_flow": "A caller invokes increment(value), which returns value + 1.",
        }
        for _ in range(20):
            queue = doc("summary_queue")
            if not queue["remaining"]:
                break
            assert queue["items"], queue
            for item in queue["items"]:
                context = doc("summary_context", id=item["id"])
                assert context["ready"]
                doc("write_summary", id=item["id"], expected_hash=context["hash"], summary=summaries[item["level"]])
        else:
            raise AssertionError("Summary queue did not complete")
        assert "returns value + 1" in doc("read_artifact", name="ATLAS.md")["content"]
        print("PASS published documentation app: real Python scan, symbols/lines and bottom-up summaries", flush=True)

        cli("daemon", "start")
        daemon = True
        other = Client(root, actor="reviewer", response_timeout=60)
        message = client.call(messaging, "action_send_direct", {"recipient": "reviewer", "body": "Public installation verified", "message_key": "public-acceptance"})
        assert other.call(messaging, "action_inbox", {})["items"][0]["message"]["id"] == message["id"]
        cli("service", "stop", messaging)
        cli("service", "start", messaging)
        assert other.call(messaging, "action_inbox", {})["items"][0]["message"]["id"] == message["id"]
        assert other.call(documentation, "action_summary_queue", {"repository": "public-fixture"})["remaining"] == 0
        print("PASS published messaging service: second-agent delivery and persistence across restart", flush=True)
    finally:
        if other:
            other.close()
        client.close()
        if daemon:
            cli("daemon", "stop")

print("PASS: both actual public packages installed through three MCP tools; anonymous uncached image pulls; simulated consent; useful code execution and persistent service state")
