"""Exercise Codex, Claude Code, Hermes and reference adapters and an installed-app hook in isolated state."""
import concurrent.futures
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile
from time import perf_counter
from universal_market_check import Client

ROOT = Path(__file__).resolve().parents[1]
BINARY = str(Path(sys.argv[1]).resolve())
PROMPT = "How are frozen provider receipts reconciled?"
GOOD = "Frozen provider receipts are reconciled by looking up the stable operation ID."
TOOL = "mcp__project__rhyven_call"


def test(home):
    env = dict(os.environ, RHYVEN_HOME=home)
    def cli(*args, payload=None):
        run = subprocess.run([BINARY,"--collection","qa",*args],input=json.dumps(payload),
                             text=True,env=env,capture_output=True)
        if run.returncode:
            raise RuntimeError(run.stderr)
        return json.loads(run.stdout)
    exported = Path(home) / "adapter.py"
    cli("context","export-adapter",str(exported))
    def adapter(mode, prompt, turn, actor="reader", timeout="4"):
        if mode=="hermes":
            event={"hook_event_name":"pre_llm_call","extra":{"user_message":prompt,"turn_id":turn}}
        elif mode=="reference":
            event={"turn_id":turn,"messages":[{"role":"user","content":prompt}]}
        else:
            event={"hook_event_name":"UserPromptSubmit","prompt":prompt,
                   ("prompt_id" if mode=="claude" else "turn_id"):turn}
        proc = subprocess.run([sys.executable,str(exported),
                               mode,"--binary",BINARY,"--collection","qa","--connection","fixture",
                               "--actor",actor,"--tool",TOOL,"--timeout",timeout],
                              input=json.dumps(event),env=env,capture_output=True,text=True,check=True)
        response=json.loads(proc.stdout)
        if mode=="hermes":
            return response.get("context")
        if mode in ("codex","claude"):
            return response.get("hookSpecificOutput",{}).get("additionalContext")
        sent=response["messages"][0]["content"]
        assert sent.startswith(prompt)
        return sent[len(prompt):].strip() or None
    def ingest(document):
        return cli("call","rhyven_call","-",payload={"category":"rhyven/wobbegong",
            "function":"action_ingest_files","args":{"source":"verification","documents":[document]}})
    doc={"id":"release","reference":"docs/release-0.9.0.md","text":GOOD}
    ingest(doc)
    assert adapter("reference",PROMPT,"off") is None
    cli("context","enable","fixture")
    start=perf_counter()
    expected=adapter("codex",PROMPT,"first")
    latency=perf_counter()-start
    assert expected and GOOD in expected and TOOL in expected,expected
    # A real Hermes gateway repeats its session ID inside a 150-character turn ID.
    session = "local-" + "a"*64
    long_id = f"{session}:{session}:12345678"
    assert len(long_id)==150
    assert adapter("hermes",PROMPT,long_id)==expected
    assert adapter("hermes",PROMPT,long_id) is None
    assert adapter("hermes",PROMPT,long_id[:-1]+"9")==expected
    for mode in ("codex","claude","hermes"):
        def diagnostics(*args):
            return cli("--actor","reader","connect","--client",mode,"--name","fixture","--check",*args)["diagnostics"]
        diag=diagnostics("--test-prompt",PROMPT)
        assert diag["actor"]=="reader" and diag["version"] and diag["executable"] and diag["collection"]=="qa"
        assert diag["enrichment"]["settings"]["enabled"]
        addition=adapter(mode,diag["enrichment"]["submit_in_harness"],"live-"+mode)
        assert GOOD in addition
        proof=re.search(r'"proof":"([^"]+)"',addition).group(1)
        cli("--actor","reader","call","rhyven_call","-",payload={"category":"rhyven/wobbegong","function":"action_confirm_excerpt","args":{"connection":"fixture","proof":proof}})
        checked=diagnostics()
        assert checked["enrichment"]["live_test"]["status"]=="confirmed" and checked["tool_alias"]==TOOL
        assert adapter(mode,diag["enrichment"]["submit_in_harness"],"live-"+mode) is None
    request=dict(connection="fixture",turn="transport",prompt=PROMPT,tool=TOOL)
    public=cli("context","preview","-",payload=request)
    workspace=Path(home)/"collections"/"qa"
    client=Client(workspace,env=env)
    try:
        assert client.call("rhyven/wobbegong","action_prepare_context",request)==public
    finally:
        client.close()
    server_env=dict(env,RHYVEN_SERVE_TOKEN="local-test-token-0123456789")
    server=subprocess.Popen([BINARY,"--collection","qa","serve","--port","0"],env=server_env,stderr=subprocess.PIPE,text=True)
    try:
        ready=json.loads(server.stderr.readline())
        bridge=Client(Path(home)/"bridge",server="http://"+ready["address"],env=server_env)
        try:
            assert bridge.call("rhyven/wobbegong","action_prepare_context",request)==public
        finally:
            bridge.close()
    finally:
        server.terminate()
        server.wait(timeout=5)
    assert adapter("reference",PROMPT,"second")==expected
    assert adapter("codex",PROMPT,"first") is None
    assert adapter("reference","orchid propagation","unrelated") is None
    assert adapter("reference","frozen provider chocolate pastry recipe","overlap") is None
    assert adapter("reference",PROMPT,"timeout",timeout="0.0001") is None
    with concurrent.futures.ThreadPoolExecutor() as pool:
        results=list(pool.map(lambda actor:adapter("codex",PROMPT,"shared-id",actor),["left","right"]))
    assert results==[expected,expected]
    src=Path(home)/"fixture-catalog"
    src.mkdir()
    cli("app","check",str(ROOT/"examples/team-customization"),"--allow-host","--out",str(src/"filter.json"))
    cli("registry-import",str(src))
    cli("install","example/team-customization","--accept-permissions")
    cli("context","enable","fixture","--app","example/team-customization","--action","select_releases")
    assert adapter("codex",PROMPT,"released")==expected
    ingest(dict(doc,reference="docs/plan-0.9.5.md"))
    assert adapter("codex",PROMPT,"planned") is None
    cli("context","enable","fixture","--clear-filter")
    ingest(dict(doc,text="# Planned\nNever shipped.\n## Released\n"+"filler text. "*345+"\nCall action_save_checkpoint here.\n## Draft\nSecret future behavior."))
    scoped=adapter("claude","action_save_checkpoint","heading-boundary")
    assert scoped and "Call action_save_checkpoint here." in scoped and " — Released " in scoped and "Draft" not in scoped and "future behavior" not in scoped,scoped
    manual=cli("wobbegong","query","action_save_checkpoint")["items"][0]
    assert manual["heading"]=="Released" and "## Draft" not in manual["text"]
    assert adapter("hermes","action_save_check","partial-identifier") is None
    cli("context","enable","fixture","--clear-filter")
    ingest({"id":"legacy.md","text":GOOD})
    assert cli("wobbegong","query","frozen provider")["items"][0]["reference"]=="legacy.md"
    assert "Source legacy.md" in adapter("hermes",PROMPT,"fallback-citation")
    cli("wobbegong","remove-source","files:verification")
    assert adapter("hermes",PROMPT,"after-source-deletion") is None
    # The diagnostic discriminates an invalid submission from a legitimate miss.
    def probe(*options):
        return cli("--actor","reader","connect","--client","hermes","--name","fixture","--check",*options)["diagnostics"]["enrichment"]
    challenge=probe("--test-prompt",PROMPT)["submit_in_harness"]
    rejected=subprocess.run([BINARY,"--collection","qa","--actor","reader","context","take","fixture","--client","hermes"],
                            input=json.dumps({"turn":"z"*129,"prompt":challenge}),text=True,capture_output=True,env=env)
    assert rejected.returncode and probe()["live_test"]["preparation"]=="invalid_submission"
    assert adapter("hermes",challenge,"no-source") is None
    assert probe()["live_test"]["preparation"]=="no_match"
    assert probe("--test-prompt",PROMPT)["live_test"]["preparation"] is None
    ingest(doc)
    cli("context","disable","fixture")
    assert adapter("reference",PROMPT,"disabled") is None
    assert cli("wobbegong","query","frozen provider")["items"]
    cli("context","enable","fixture","--clear-filter")
    assert adapter("reference",PROMPT,"enabled")
    print(json.dumps({"positive_fixtures":1,"unrelated_or_misleading":2,"false_injections":0,
                      "query_generation_model_calls":0,"codex_adapter_seconds":round(latency,3)}))


if __name__=="__main__":
    with tempfile.TemporaryDirectory() as home:
        test(home)
