"""Exercise the public 0.9.5 CLI in a disposable home with a local ticket provider."""
import concurrent.futures
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import subprocess
import sys
import tempfile
from threading import Thread
from time import sleep
from urllib.request import urlopen

BINARY = str(Path(sys.argv[1]).resolve())
WORK = "rhyven/work-management"


def run(home, actor, *argv, payload=None, check=True):
    result = subprocess.run([BINARY, "--home", home, "--collection", "qa", "--actor", actor, *argv],
                            input=json.dumps(payload), capture_output=True, text=True)
    if check and result.returncode:
        raise RuntimeError(result.stderr)
    return json.loads(result.stdout if not result.returncode else result.stderr)


def test(home):
    run(home, "setup", "install", WORK, "--accept-permissions")

    def call(actor, function, args, check=True):
        return run(home, actor, "call", "rhyven_call", "-", check=check,
                   payload={"category": WORK, "function": function, "args": args})

    task = call("planner", "object_task_create", {"data": {"title": "Record the regression"}})["id"]
    with concurrent.futures.ThreadPoolExecutor() as pool:
        attempts = list(pool.map(lambda actor: call(actor, "action_claim", {"id":task,"ttl":120},False), ["a","b"]))
    assert sum("token" in result for result in attempts) == 1, attempts
    index = next(i for i,res in enumerate(attempts) if "token" in res)
    actor, successor = ("a","b") if index == 0 else ("b","a")
    claim = {"id":task,"token":attempts[index]["token"]}
    assert call(actor,"action_ownership",{})["items"][0]["current_owner"] == actor
    call(actor,"action_save_checkpoint",{**claim,"checkpoint":{"completed":"Prepared ticket","next":"Verify ticket"}})
    intent = {"key":"ticket","arguments":'{"title":"regression"}'}
    original={**claim,**intent,"request_id":"dispatch-ticket"}
    reservation = call(actor,"action_operation_intent",original)
    assert reservation["next_action"]=="dispatch" and reservation["dispatch"]
    assert call(actor,"action_operation_intent",original)["next_action"]=="reconcile"
    op = reservation["operation_id"]

    tickets = {}
    class Provider(BaseHTTPRequestHandler):
        def do_POST(self):
            key = self.headers["Idempotency-Key"]
            tickets.setdefault(key,"ticket-1")
            self.send_response(200); self.end_headers(); self.wfile.write(json.dumps({"ticket":tickets[key]}).encode())
        def do_GET(self):
            self.send_response(200); self.end_headers(); self.wfile.write(tickets[self.path[1:]].encode())
        def log_message(self,*args):
            pass
    with ThreadingHTTPServer(("127.0.0.1",0),Provider) as provider:
        worker = Thread(target=provider.serve_forever,daemon=True)
        worker.start()
        try:
            url = f"http://127.0.0.1:{provider.server_port}/"
            spec=Path(home)/"tickets-openapi.json"
            spec.write_text(json.dumps({"openapi":"3.0.0","paths":{"/tickets":{"post":{
                "operationId":"create_ticket","summary":"Create one ticket",
                "parameters":[{"name":"Idempotency-Key","in":"header","required":True,"schema":{"type":"string"}}],
                "responses":{"200":{"description":"Ticket"}}}}}}))
            packages=Path(home)/"fixture-packages"
            packages.mkdir()
            run(home,"setup","app","import-openapi",str(spec),"--name","example/tickets",
                "--endpoint",url.rstrip("/"),"--include","create_ticket","--out",str(packages/"ticket.json"))
            run(home,"setup","registry-import",str(packages))
            run(home,"setup","install","example/tickets","--accept-permissions")
            run(home,actor,"call","rhyven_call","-",payload={"category":"example/tickets",
                "function":"action_create_ticket","args":{"idempotency_key":op}})
            call(actor,"action_renew_claim",{**claim,"ttl":1})
            # First process exited without recording the response.
            sleep(1.1)
            takeover = call(successor,"action_claim",{"id":task})
            assert takeover["handoff"]["checkpoint"]["next"] == "Verify ticket"
            fresh = {"id":task,"token":takeover["token"]}
            pending = call(successor,"action_operation_intent",{**fresh,**intent})
            assert pending["next_action"]=="reconcile" and pending["operation_id"] == op and pending["needs_reconciliation"] and not pending["dispatch"]
            with urlopen(url+op) as response:
                reference = response.read().decode()
            call(successor,"action_operation_resolve",{**fresh,"key":"ticket","reference":reference})
            assert call(actor,"action_operation_intent",original)["next_action"]=="use_recorded_result"
            recorded=call(successor,"action_operation_intent",{**fresh,**intent})
            assert recorded["next_action"]=="use_recorded_result" and recorded["reference"]==reference
            call(successor,"action_complete_claim",{**fresh,"result":"Recovered "+reference})
            assert call(actor,"action_complete_claim",{**claim,"result":"late"},False)["code"] == "claim_conflict"
            # A provider without lookup or replay support must remain unresolved.
            other=call("planner","object_task_create",{"data":{"title":"Unverified endpoint"}})["id"]
            first=call(actor,"action_claim",{"id":other})
            old={"id":other,"token":first["token"]}
            call(actor,"action_operation_intent",{**old,**intent})
            revision=call(actor,"object_task_get",{"id":other})["revision"]
            run(home,"operator","reassign-task",other,"--expected-revision",str(revision),"--owner",successor)
            assert call(actor,"action_save_checkpoint",{**old,"checkpoint":{"completed":"late","next":"bad"}},False)["code"]=="claim_conflict"
            assert call(actor,"action_claim",{"id":other},False)["code"]=="claim_conflict"
            takeover2=call(successor,"action_claim",{"id":other})
            unresolved=call(successor,"action_operation_intent",{"id":other,"token":takeover2["token"],**intent})
            assert not unresolved["dispatch"] and unresolved["needs_reconciliation"]
            events=run(home,"operator","call","rhyven_call","-",payload={"category":"rhyven/marketplace",
                "function":"action_changes","args":{"app":WORK,"after":0,"limit":100}})
            assert events["contract_version"]==1 and not events["has_more"]
            assert {"claim","save_checkpoint","transfer_claim","complete_claim"} <= {e["action"] for e in events["items"]}
            assert first["token"] not in json.dumps(events)
            project=Path(__file__).resolve().parents[1]
            run(home,"setup","app","check",str(project/"examples/team-customization"),"--allow-host",
                "--out",str(packages/"customization.json"))
            run(home,"setup","registry-import",str(packages))
            run(home,"setup","install","example/team-customization","--accept-permissions")
            run(home,"setup","compose","--definition",str(project/"examples/team-customization/ownership-workflow.json"),
                "--out",str(packages/"report.json"))
            run(home,"setup","registry-import",str(packages))
            run(home,"setup","install","example/ownership-report","--accept-permissions")
            report=run(home,"observer","call","rhyven_call","-",payload={"category":"example/ownership-report",
                "function":"action_group","args":{"request_id":"report-1","offset":0}})
            groups={row["owner"]:row["tasks"] for row in report["groups"]}
            assert "Record the regression" in groups[None] and "Unverified endpoint" in groups[successor]
            assert len(tickets)==1
            row = next(row for row in call(successor,"action_ownership",{})["items"] if row["id"]==task)
            assert row["status"]=="done" and row["current_owner"] is None
            assert call(successor,"action_handoff",{"id":task})["operations"][0]["reference"]==reference
        finally:
            provider.shutdown()
            worker.join()


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as home:
        test(home)
    print("Concurrent claims, checkpoint takeover, provider reconciliation and stale completion passed.")
