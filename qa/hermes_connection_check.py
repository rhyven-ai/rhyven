"""Optional upstream Hermes smoke test. Run with Hermes and its MCP extra on PYTHONPATH."""
import asyncio
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory(prefix="rhyven-hermes-") as temporary:
    root = Path(temporary)
    os.environ["HERMES_HOME"] = str(root / "hermes")
    config = root / "hermes/config.yaml"
    config.parent.mkdir()
    config.write_text('# Existing settings\nlegacy_flag: on\nmcp_servers:\n  disabled_test:\n    command: unused\n    enabled: no\n')
    result = subprocess.run([binary, "--home", str(root / "state"), "--collection", "hermes-test",
                             "connect", "--client", "hermes"], check=True, capture_output=True, text=True)
    assert json.loads(result.stdout)["server_verified"]
    from hermes_cli.config import load_config
    from tools.mcp_tool_config import _load_mcp_config
    from tools.mcp_tool import MCPServerTask
    parsed = load_config()
    assert parsed['legacy_flag'] is True
    assert parsed['mcp_servers']['disabled_test']['enabled'] is False

    async def check():
        server = MCPServerTask("rhyven")
        try:
            await asyncio.wait_for(server.start(_load_mcp_config()["rhyven"]), 45)
            tools = await server.session.list_tools()
            assert sorted(tool.name for tool in tools.tools) == ["rhyven_call", "rhyven_categories", "rhyven_describe"]
            result = await server.session.call_tool("rhyven_categories", {})
            value = result.model_dump(by_alias=True)
            assert not value.get("isError", False), value
            discovery = value.get("structuredContent") or json.loads(value["content"][0]["text"])
            assert discovery["collection"] == "hermes-test", discovery
            result = await server.session.call_tool("rhyven_describe", {"category": "rhyven/marketplace"})
            assert not result.model_dump(by_alias=True).get("isError", False)
        finally:
            await server.shutdown()
    asyncio.run(check())
print("PASS: upstream Hermes loads merged YAML, preserves booleans, discovers three tools, and reads the selected collection and marketplace")
