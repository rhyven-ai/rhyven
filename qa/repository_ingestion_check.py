"""Test Repository File Ingestion and collection search through released interfaces.

Usage: python3 qa/repository_ingestion_check.py BINARY PATH_TO_FILE_INGESTION_APP
"""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

BINARY, SOURCE = (str(Path(arg).resolve()) for arg in sys.argv[1:])
INGESTION = "rhyven/file-rag"
SEARCH = "rhyven/wobbegong"


def test(home):
    def cli(*cmd, collection="piers", payload=None, error=None):
        proc = subprocess.run([BINARY, "--home", str(home), "--collection", collection, *map(str, cmd)],
                              input=json.dumps(payload), text=True, capture_output=True, timeout=45)
        if error:
            assert proc.returncode != 0 and json.loads(proc.stderr)["code"] == error, proc.stderr
            return
        assert proc.returncode == 0, proc.stderr
        return json.loads(proc.stdout)

    def call(category, function, args, **kw):
        return cli("call", "rhyven_call", "-", payload={"category": category, "function": function, "args": args}, **kw)

    def preview(turn, prompt):
        return cli("context", "preview", "-", payload={
            "connection": "bridge", "turn": turn, "prompt": prompt, "tool": "lookup"})

    package = home / "ingestion.json"
    cli("app", "check", SOURCE, "--allow-host", "--out", package)
    cli("install", package, "--accept-permissions")
    discover = cli("call", "rhyven_categories", "-", payload={})["apps"]
    names = {item["name"]: item["display_name"] for item in discover}
    for category, label in [(INGESTION, "Repository File Ingestion"), (SEARCH, "Wobbegong Collection Search")]:
        assert names[category] == label
        assert cli("call", "rhyven_describe", "-", payload={"category": category})["display_name"] == label
    cli("context", "enable", "bridge")
    args = {"repository": "harbor"}
    files = [{"path": "ops/radio.md", "content": "# Radio shutdown\nThe teal antenna amplifier controls radio shutdown."},
             {"path": "ops/draft.md", "content": "The draft radio shutdown uses the prototype."}]
    call(INGESTION, "action_put_files", dict(args, files=files))
    # Force separate export pages without exceeding the host's per-action I/O limit.
    for n in range(6):
        call(INGESTION, "action_put_files", dict(args, files=[
            {"path": f"ops/{n}.txt", "content": "navigation " * 14_000}
        ]))
    selection = dict(args, root="ops", exclude=["draft*"])
    page = call(INGESTION, "action_export_documents", dict(selection, offset=0))
    assert 0 < page["next_offset"] < 7
    exported = page["documents"] + call(INGESTION, "action_export_documents",
                                        dict(selection, offset=page["next_offset"]))["documents"]
    assert len(exported) == len({d["id"] for d in exported}) == 7
    result = cli("wobbegong", "ingest-repository", "harbor", "--root", "ops", "--exclude", "draft*")
    assert result["committed"] and result["documents"] == 7
    source = "app:rhyven/file-rag:harbor/ops"
    assert result["source"] == source
    status = cli("context", "status", "bridge")["index"]
    assert status["category"] == SEARCH and status["sources"][0]["source"] == source
    manual = cli("wobbegong", "query", "teal antenna amplifier")["items"][0]
    match = preview("first", "Where is the teal antenna amplifier?")
    assert manual["reference"] == match["excerpts"][0]["reference"] == "ops/radio.md"
    assert "prototype" not in match["addition"]
    assert not cli("wobbegong", "query", "teal antenna amplifier", collection="elsewhere")["items"]
    call(INGESTION, "action_put_files", dict(args, files=[], delete=["ops/radio.md"]))
    cli("wobbegong", "ingest-repository", "harbor", "--root", "ops", "--exclude", "!bad", error="app_error")
    assert "teal antenna amplifier" in preview("failed-refresh", "Where is the teal antenna amplifier?")["addition"]
    result = call(SEARCH, "action_ingest_repository", selection)
    assert result["documents"] == 6 and not cli("wobbegong", "query", "teal antenna amplifier")["items"]
    assert preview("after-file-delete", "Where is the teal antenna amplifier?")["addition"] is None

    knowledge = "rhyven/project-knowledge"
    cli("install", knowledge, "--accept-permissions")
    old = call(knowledge, "action_remember", {"title": "Mooring key", "body": "The amber mooring key is ORANGE.", "topic": "port"})
    cli("wobbegong", "ingest-records", knowledge, "note", "--fields", "title,body")
    prompt = "Where is the amber mooring key?"
    assert "ORANGE" in preview("before-correction", prompt)["addition"]
    call(knowledge, "action_correct", {"title": "Mooring key", "body": "The amber mooring key is SILVER.",
                                      "topic": "port", "supersedes": old["id"]})
    assert cli("wobbegong", "query", "amber mooring key")["items"][0]["stale"] is True
    assert preview("corrected", prompt)["addition"] is None
    cli("wobbegong", "ingest-records", knowledge, "note", "--fields", "title,body")
    current = preview("reindexed", prompt)["addition"]
    assert "SILVER" in current and "ORANGE" not in current
    print("Shared index passed: paged import, cited search, exclusions, collection isolation, failed refresh, deletion and supersession.")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as temp:
        test(Path(temp))
