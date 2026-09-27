"""Real terminal refresh while another CLI client installs, publishes and removes.

Usage: PYTHONPATH=/path/to/pyte python3 qa/tui_refresh_check.py BINARY OUTPUT.json
All state is temporary. No registry/network access or Docker required.
"""
import codecs
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time

import pyte

binary = str(Path(sys.argv[1]).resolve())
output = Path(sys.argv[2])
with tempfile.TemporaryDirectory(prefix="rhyven-tui-refresh-") as workspace:
    def cli(*args):
        result = subprocess.run([binary, "--workspace", workspace, *args], capture_output=True, text=True, timeout=30)
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)

    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 38, 120, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([binary, "--workspace", workspace], stdin=slave, stdout=slave, stderr=slave,
                               env=dict(os.environ, TERM="xterm-256color", COLORTERM="truecolor"))
    screen = pyte.Screen(120, 38)
    stream = pyte.Stream(screen)
    decoder = codecs.getincrementaldecoder("utf-8")("replace")

    def text():
        return "\n".join(screen.display)

    def pump(seconds=.2):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if select.select([master], [], [], .05)[0]:
                data = os.read(master, 65536)
                if not data:
                    break
                stream.feed(decoder.decode(data))

    def wait_for(fragment, timeout=7):
        deadline = time.monotonic() + timeout
        while fragment not in text() and time.monotonic() < deadline:
            pump()
        assert fragment in text(), f"Missing {fragment!r}:\n{text()}"

    def press(keys):
        os.write(master, keys)
        pump()

    def version(name):
        return next(app["version"] for app in cli("list") if app["name"] == name)

    try:
        wait_for("Auto-refresh 2s")
        press(b"\t")
        wait_for("No matching apps")
        press(b"?")
        wait_for("Refresh and app updates")
        assert "does not install updates or contact GitHub" in text()
        press(b"\x1b")

        # Do not send any TUI input while waiting for external changes.
        cli("install", "rhyven/work-management", "--accept-permissions")
        wait_for("Work Management")
        cli("install", "rhyven/inventory", "--accept-permissions")
        wait_for("Inventory")
        wait_for("2 apps")
        press(b"\r")
        wait_for("App details")
        assert "rhyven/work-management" in text(), "External install stole the selected app"
        press(b"\x1b")

        package = cli("inspect", "rhyven/inventory")["package"]
        old_version = version("rhyven/inventory")
        package["version"] = "0.4.0"
        path = Path(workspace) / "new-inventory.json"
        path.write_text(json.dumps(package))
        cli("app", "publish", str(path))
        wait_for(f"v{old_version} → v0.4.0")
        assert version("rhyven/inventory") == old_version, "Refresh installed an update"
        press(b"ku")
        wait_for("Review app update")
        assert "recovery backup" in text()
        assert "Added permissions: none" in text()
        cells = [[{"text": screen.buffer[y][x].data, "fg": screen.buffer[y][x].fg,
                   "bg": screen.buffer[y][x].bg, "bold": screen.buffer[y][x].bold}
                  for x in range(120)] for y in range(38)]
        output.write_text(json.dumps({"columns": 120, "rows": 38, "display": screen.display, "cells": cells}))
        press(b"n")
        assert version("rhyven/inventory") == old_version
        press(b"uy")
        wait_for("Updated rhyven/inventory to 0.4.0")
        assert version("rhyven/inventory") == "0.4.0"
        assert list((Path(workspace) / ".rhyven/recovery").iterdir()), "Update must retain its backup"

        cli("remove", "rhyven/inventory")
        wait_for("1 apps")
        assert "Inventory" not in "\n".join(screen.display[15:31])
        press(b"q")
        assert process.wait(timeout=5) == 0
        assert termios.tcgetattr(slave) == original, "Terminal mode was not restored"
        print("PASS: idle TUI refresh sees external install, update availability and removal; selection retained; update consent, backup and help verified.")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)
