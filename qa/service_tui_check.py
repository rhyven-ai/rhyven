"""Real terminal lifecycle controls. Requires pyte and a locally built service image.
Usage: python3 qa/service_tui_check.py BINARY IMAGE_ID_FILE
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
app = "example/background-counter"
with tempfile.TemporaryDirectory(prefix="rv-ui-") as tmp:
    home = Path(tmp)
    def cli(*args):
        value = subprocess.run([binary,"--home",tmp,*args],text=True,capture_output=True,timeout=90)
        assert not value.returncode, value.stderr
        return json.loads(value.stdout)
    p = json.loads(Path("examples/container-service-python/app.json").read_text())
    p["execution"]["image"] = Path(sys.argv[2]).read_text().strip()
    bundle = home/"app.json"
    bundle.write_text(json.dumps(p))
    cli("install",str(bundle),"--accept-permissions")
    cli("daemon","start")
    master,slave = pty.openpty()
    fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack("HHHH",38,120,0,0))
    proc = subprocess.Popen([binary,"--home",tmp],stdin=slave,stdout=slave,stderr=slave,
        env=dict(os.environ,TERM="xterm-256color"))
    screen = pyte.Screen(120,38)
    stream = pyte.Stream(screen)
    decoder = codecs.getincrementaldecoder("utf-8")("replace")
    def pump(seconds=.3):
        until = time.monotonic()+seconds
        while time.monotonic()<until:
            if select.select([master],[],[],.05)[0]:
                stream.feed(decoder.decode(os.read(master,65536)))
    try:
        pump()
        os.write(master,b"\t")
        pump()
        assert "background-counter" in "\n".join(screen.display)
        os.write(master,b"b")
        for _ in range(60):
            pump()
            if "Service status / logs" in "\n".join(screen.display): break
        assert cli("service","status",app)["state"] == "ready"
        assert "Service status / logs" in "\n".join(screen.display)
        os.write(master,b"\x1b")
        pump()
        os.write(master,b"l")
        pump()
        assert "Service status / logs" in "\n".join(screen.display)
        os.write(master,b"\x1b")
        pump()
        os.write(master,b"t")
        pump(3)
        assert cli("service","status",app)["desired"] == "stopped"
        os.write(master,b"\x1bq")
        pump()
        assert proc.wait(timeout=5)==0
        print("PASS: real TUI installed service start, scrollable diagnostics/logs and durable stop")
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()
        os.close(master)
        os.close(slave)
        cli("daemon","stop")
        until=time.monotonic()+30
        while (home/"supervisor/control.sock").exists() and time.monotonic()<until:
            time.sleep(.1)
        assert not (home/"supervisor/control.sock").exists()
