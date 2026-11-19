"""Two example app actions using only documented Rhyven data contracts."""
import json
from pathlib import PurePosixPath
import sys

request = json.load(sys.stdin)
args = request["args"]
if request["function"] == "action_select_releases":
    selected = []
    for excerpt in args["excerpts"]:
        name = PurePosixPath(excerpt["reference"]).name
        if name.startswith("release-") and name.endswith(".md"):
            selected.append(excerpt["id"])
    result = {"selected": selected}
elif request["function"] == "action_group_owners":
    owners = {}
    for item in args["items"]:
        owners.setdefault(item["current_owner"], []).append(item["task"])
    result = {"groups": [{"owner": owner, "tasks": tasks} for owner, tasks in owners.items()]}
else:
    raise ValueError("Unknown action")
print(json.dumps({"result": result}))
