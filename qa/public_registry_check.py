"""Anonymous company-registry acceptance using the three-tool MCP interface.

python3 qa/public_registry_check.py BINARY
Uses isolated temporary state and simulated host consent; no GitHub writes.
"""
import os
from pathlib import Path
import tempfile

from universal_market_check import Client, cli, market


for key in ("GH_TOKEN", "GITHUB_TOKEN"):
    os.environ.pop(key, None)

with tempfile.TemporaryDirectory(prefix="rhyven-public-registry-") as folder:
    root = Path(folder)
    refreshed = cli(root, "registry-refresh", "rhyven-ai/registry", "--anonymous")
    assert refreshed["package_downloads"] == 0
    assert cli(root, "list") == []
    client = Client(root)
    try:
        apps = ("work-management", "project-knowledge", "error-management", "ci-management", "inventory")
        for name in apps:
            category = "rhyven/" + name
            listing = client.call(market, "object_listing_get", {"id": category})["data"]
            assert listing["repository"] == "rhyven-ai/registry", listing
            assert listing["publisher_label"] == "Rhyven", listing
            assert listing["display_name"] and listing["trust"] == "Unverified", listing
            assert listing["stars_status"] == "GitHub repository stars (cached)" and isinstance(listing["stars"], int), listing
            request = client.call(market, "action_prepare_install", {"app": category})
            assert request["repository"] == "rhyven-ai/registry", request
            client.call(market, "action_apply", {"request_id": request["request_id"]}, consent=False, error="approval_required")
            assert not any(app["name"] == category for app in cli(root, "list"))
            request = client.call(market, "action_prepare_install", {"app": category})
            client.call(market, "action_apply", {"request_id": request["request_id"]}, consent=True)
            description = client.tool("rhyven_describe", {"category": category})
            assert description["functions"] and description["guidance_markdown"]

        work, knowledge = "rhyven/work-management", "rhyven/project-knowledge"
        task = client.call(work, "object_task_create", {"data": {"title": "Verify public distribution", "labels": ["release"]}})
        note = client.call(knowledge, "action_remember", {"title": "Public registry verified", "body": "Installed through anonymous GitHub downloads and MCP host consent.", "topic": "release"})
        assert client.call(work, "object_task_query", {"search": "public distribution"})["total"] == 1
        assert client.call(knowledge, "object_note_get", {"id": note["id"]}) == note
        remove = client.call(market, "action_prepare_remove", {"app": work})
        assert client.call(market, "action_apply", {"request_id": remove["request_id"]}, consent=True)["data_retained"]
        reinstall = client.call(market, "action_prepare_install", {"app": work})
        client.call(market, "action_apply", {"request_id": reinstall["request_id"]}, consent=True)
        assert client.call(work, "object_task_get", {"id": task["id"]}) == task
    finally:
        client.close()

    other_agent = Client(root, actor="reviewer")
    try:
        assert other_agent.call(work, "object_task_get", {"id": task["id"]}) == task
        assert other_agent.call(knowledge, "object_note_get", {"id": note["id"]}) == note
    finally:
        other_agent.close()

print("PASS: anonymous public catalog/stars, five Rhyven app installs, denied/simulated host consent, three-tool discovery and use, retained-state reinstall and second-agent persistence")
