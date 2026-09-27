"""Test-only service; copied beside the example as fault.py during acceptance."""
import os
import sys
import time
import main as counter
from rhyven_service import Service


def probe(args, context, service):
    try:
        result = service.call(args["category"], args["function"], args.get("args", {}))
        return {"allowed": True, "result": result}
    except RuntimeError as error:
        return {"allowed": False, "error": str(error)}


def crash(args, context, service):
    os._exit(17)


def hang(args, context, service):
    time.sleep(60)
    return {}


def flood(args, context, service):
    sys.stdout.write("x" * 1_100_000 + "\n")
    sys.stdout.flush()
    return {}


def log_flood(args, context, service):
    sys.stderr.write("l" * 100_000 + "\n")
    sys.stderr.flush()
    return {}


def bad(args, context, service):
    return {"wrong": "type"}


def fail(args, context, service):
    raise ValueError("Deliberately failed health check")


def delayed(args, context, service):
    time.sleep(.5)
    return counter.remember({"title": "Delayed", "body": "Maintenance race"}, context, service)


Service({"action_status": counter.status, "action_increment": counter.increment,
         "action_remember": counter.remember, "action_health": counter.health,
         "action_probe": probe, "action_crash": crash, "action_hang": hang,
         "action_flood": flood, "action_log_flood": log_flood,
         "action_bad": bad, "action_fail": fail, "action_delayed": delayed},
        start=counter.start, stop=counter.stop).run()
