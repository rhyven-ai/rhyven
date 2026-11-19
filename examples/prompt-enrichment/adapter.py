"""Reference pre-submit adapters. Input and output are JSON on standard I/O."""
import argparse
import hashlib
import json
import subprocess
import sys
import uuid


def prepare(options, prompt, turn):
    # Reserve this prefix so a short native ID cannot collide with a mapped ID.
    if isinstance(turn, str) and (len(turn) > 128 or turn.startswith("rhyven-sha256:")):
        turn = "rhyven-sha256:" + hashlib.sha256(turn.encode("utf-8")).hexdigest()
    request = dict(prompt=prompt, turn=turn)
    if options.tool:
        request["tool"] = options.tool
    command = [options.binary, "--collection", options.collection, "--actor", options.actor,
               "context", "take", options.connection, "--client", options.mode]
    if options.home:
        command[1:1] = ["--home",options.home]
    try:
        result = subprocess.run(command, input=json.dumps(request), text=True,
                                capture_output=True, check=True, timeout=options.timeout)
        return json.loads(result.stdout).get("addition")
    except (subprocess.SubprocessError, OSError, ValueError):
        return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=["codex", "claude", "hermes", "reference"])
    parser.add_argument("--binary", required=True)
    parser.add_argument("--collection", required=True)
    parser.add_argument("--home")
    parser.add_argument("--actor", required=True)
    parser.add_argument("--connection", required=True)
    parser.add_argument("--tool", help="Override the host tool alias")
    parser.add_argument("--timeout", type=float, default=2.0)
    options = parser.parse_args()
    event = json.load(sys.stdin)
    if options.mode == "hermes":
        extra = event.get("extra") or {}
        if event.get("hook_event_name") != "pre_llm_call" or not isinstance(extra.get("user_message"), str):
            return {}
        turn = extra.get("turn_id") or str(uuid.uuid4())
        addition = prepare(options, extra["user_message"], turn)
        return {"context": addition} if addition else {}
    if options.mode in ("codex", "claude"):
        if event.get("hook_event_name") != "UserPromptSubmit" or not isinstance(event.get("prompt"), str):
            return {}
        turn = (event.get("prompt_id") or str(uuid.uuid4())) if options.mode == "claude" else event.get("turn_id")
        if not turn:
            return {}
        addition = prepare(options, event["prompt"], turn)
        if addition:
            return {"hookSpecificOutput": {"hookEventName": "UserPromptSubmit",
                                           "additionalContext": addition}}
        return {}
    # Called by the host at submission, before forwarding its model request.
    message = event["messages"][-1]
    if message.get("role") != "user" or not isinstance(message.get("content"), str):
        return event
    addition = prepare(options, message["content"], event["turn_id"])
    if addition:
        message["content"] += "\n\n" + addition
    return event


if __name__ == "__main__":
    print(json.dumps(main(), ensure_ascii=False))
