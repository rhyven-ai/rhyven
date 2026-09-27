import json
import os
import socket
import sys
import time

request = json.loads(sys.stdin.readline())
if request["function"] in ["action_migrate", "action_fail", "action_health"]:
    from pathlib import Path
    data = Path(os.environ["RHYVEN_DATA_DIR"])
    if request["function"] != "action_health":
        (data / "migration.json").write_text(json.dumps(request["args"]))
    if request["function"] == "action_fail":
        print(json.dumps({"error": {"code": "test_failure", "message": "Fail after writing staged data"}}))
    else:
        print(json.dumps({"result": {"ok": (data / "migration.json").exists()}}))
    sys.exit(0)
mode = request["args"]["mode"]
if mode == "timeout":
    time.sleep(60)
elif mode == "overflow":
    print("x" * 1048577)
elif mode == "invalid":
    print("this is not JSON")
else:
    def denied(path):
        try:
            with open(path, "w") as f:
                f.write("probe")
            return False
        except OSError:
            return True
    try:
        with socket.create_connection(("1.1.1.1", 443), timeout=.2):
            network_denied = False
    except OSError:
        network_denied = True
    print(json.dumps({"result": {
        "root_readonly": denied("/app/forbidden"),
        "data_readonly": denied("/data/probe"),
        "network_denied": network_denied,
        "declared_secret": "RHYVEN_SECRET_TEST" in os.environ,
        "undeclared_secret": "RHYVEN_SECRET_UNDECLARED" in os.environ,
    }}))
