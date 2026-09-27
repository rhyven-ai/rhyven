"""Optional verification only. Product runtime is entirely Rust.
Requires: mcp==1.26.0 jsonschema. Usage: python qa/mcp_sdk_check.py /absolute/rhyven
"""
import asyncio
import json
from pathlib import Path
import subprocess
import sys
import tempfile

from jsonschema import Draft202012Validator
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

binary = str(Path(sys.argv[1]).resolve())


def command(root, *args):
    return json.loads(subprocess.check_output([binary, "--workspace", str(root), *args]))


def value(result):
    assert not result.isError, result
    return json.loads(result.content[0].text)


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


async def universal(session, name, arguments):
    tool, args = universal_arguments(name, arguments)
    return await session.call_tool(tool, args)


async def main():
    with tempfile.TemporaryDirectory(prefix="rhyven-sdk-") as temp:
        root = Path(temp) / "workspace"
        parameters = StdioServerParameters(command=binary, args=["--workspace", str(root), "mcp"])
        async with stdio_client(parameters) as (read, write):
            async with ClientSession(read, write) as session:
                init = await session.initialize()
                tools = (await session.list_tools()).tools
                assert {t.name for t in tools} == {"rhyven_categories", "rhyven_describe", "rhyven_call"}
                for tool in tools:
                    Draft202012Validator.check_schema(tool.inputSchema)
                assert [a["name"] for a in value(await universal(session, "list_apps", {}))["apps"]] == ["rhyven/marketplace", "rhyven/runtime"]
                for package in ["rhyven/work-management", "rhyven/project-knowledge", "rhyven/error-management", "rhyven/ci-management", "rhyven/inventory"]:
                    command(root, "install", package, "--accept-permissions")
                assert len(value(await universal(session, "list_apps", {}))["apps"]) == 7
                assert [t.name for t in (await session.list_tools()).tools] == [t.name for t in tools]
                contract = value(await universal(session, "describe_app", {"app": "rhyven/work-management"}))
                assert "task" in contract["contract"]["objects"]
                args = {"app": "rhyven/work-management", "object": "task", "data": {"title": "SDK workflow"}, "request_id": "sdk-task"}
                task = value(await universal(session, "create", args))
                assert value(await universal(session, "create", args)) == task
                task = value(await universal(session, "execute", {"app": task["app"], "action": "assign", "args": {"id": task["id"], "expected_revision": task["revision"], "owner": "sdk-agent"}}))
                note = value(await universal(session, "execute", {"app": "rhyven/project-knowledge", "action": "remember", "args": {"title": "Validation", "body": "Official SDK can use the universal runtime."}}))
                assert note["data"]["kind"] == "note"
                task = value(await universal(session, "execute", {"app": task["app"], "action": "complete", "args": {"id": task["id"], "expected_revision": task["revision"], "result": "Verified"}}))
                assert task["data"]["status"] == "done"
                bad = await universal(session, "update", {"app": task["app"], "object": "task", "id": task["id"], "expected_revision": 1, "patch": {"title": "stale"}})
                assert bad.isError
        export = Path(temp) / "standalone"
        command(root, "export-mcp", "rhyven/inventory", "--out", str(export))
        async with stdio_client(StdioServerParameters(command=str(export / "launch.sh"))) as (read, write):
            async with ClientSession(read, write) as session:
                await session.initialize()
                tools = (await session.list_tools()).tools
                for tool in tools:
                    Draft202012Validator.check_schema(tool.inputSchema)
                asset = value(await session.call_tool("object_asset_create", {"data": {"label": "Independent", "serial": "42"}}))
                retired = value(await session.call_tool("action_retire", {"args": {"id": asset["id"], "expected_revision": 1}}))
                assert retired["data"]["status"] == "retired"
        print(json.dumps({"sdk": "mcp 1.26.0", "protocol": init.protocolVersion, "universal_tools": 3, "live_install": "passed", "workflow": "passed", "standalone": "passed", "schemas": "valid"}))


asyncio.run(main())
