"""Exercise actual terminal I/O. Optional QA dependency: pyte."""
import codecs
import fcntl
import json
import os
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path
import pyte

binary, output = sys.argv[1:]
binary = str(Path(binary).resolve())
with tempfile.TemporaryDirectory(prefix="rhyven-tui-") as workspace:
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 38, 120, 0, 0))
    original = termios.tcgetattr(slave)
    process = subprocess.Popen([binary, "--workspace", workspace], stdin=slave, stdout=slave, stderr=slave, env=dict({k:v for k,v in os.environ.items() if k != "NO_COLOR"}, TERM="xterm-256color", COLORTERM="truecolor"))
    screen = pyte.Screen(120, 38)
    stream = pyte.Stream(screen)
    decoder = codecs.getincrementaldecoder("utf-8")("replace")

    def pump(seconds=0.4):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if select.select([master], [], [], 0.05)[0]:
                data = os.read(master, 65536)
                if not data:
                    break
                stream.feed(decoder.decode(data))

    pump()
    os.write(master, b"/work-management\r")
    pump()
    assert "Work Management" in "\n".join(screen.display)
    # Click the first result (SGR mouse coordinates are one-based).
    os.write(master, b"\x1b[<0;30;17M\x1b[<0;30;17m")
    pump()
    assert "App details" in "\n".join(screen.display)
    description = json.loads(subprocess.check_output([binary, "--workspace", workspace, "inspect", "rhyven/work-management"]))["package"]["description"]
    assert description[:35] in "\n".join(screen.display)
    assert json.loads(subprocess.check_output([binary, "--workspace", workspace, "list"])) == []
    os.write(master, b"\x1b")
    pump()
    os.write(master, b"i")
    pump()
    assert "Review installation" in "\n".join(screen.display)
    assert json.loads(subprocess.check_output([binary, "--workspace", workspace, "list"])) == []
    os.write(master, b"n")
    pump()
    assert json.loads(subprocess.check_output([binary, "--workspace", workspace, "list"])) == []
    os.write(master, b"iy")
    pump()
    assert "Installed" in "\n".join(screen.display)
    os.write(master, b"/\rjjjj")
    pump()
    assert "Headless apps for agents" not in "\n".join(screen.display)
    assert "TOMORROW" not in "\n".join(screen.display)
    cells = [[{"text": screen.buffer[y][x].data, "fg": screen.buffer[y][x].fg, "bg": screen.buffer[y][x].bg, "bold": screen.buffer[y][x].bold} for x in range(120)] for y in range(38)]
    Path(output).write_text(json.dumps({"columns": 120, "rows": 38, "display": screen.display, "cells": cells}))
    os.write(master, b"s")
    pump()
    assert "App contract" in "\n".join(screen.display)
    os.write(master, b"g")
    pump()
    assert "Agent guide" in "\n".join(screen.display)
    os.write(master, b"q")
    pump()
    assert process.wait(timeout=5) == 0
    assert original == termios.tcgetattr(slave), "Terminal mode was not restored"
    installed = json.loads(subprocess.check_output([binary, "--workspace", workspace, "list"]))
    assert [p["name"] for p in installed] == ["rhyven/work-management"]
    os.close(master)
    os.close(slave)
    print("PTY search, review, cancellation, install, contract, guide, quit and terminal restoration: passed")
