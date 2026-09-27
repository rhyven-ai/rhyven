"""Linux memory-bound proof: back up and restore 128 MiB with a 96 MiB process limit.
Usage: python3 qa/streaming_backup_check.py target/release/rhyven
"""
import hashlib
import json
from pathlib import Path
import resource
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1]).resolve())
limit = 96 * 1024 * 1024

def memory_limit():
    resource.setrlimit(resource.RLIMIT_AS, (limit, limit))

def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(65536):
            result.update(block)
    return result.hexdigest()

with tempfile.TemporaryDirectory(prefix="rhyven-streaming-") as temporary:
    home = Path(temporary)
    def cli(*args):
        result = subprocess.run([binary, "--home", str(home), *args],
                                preexec_fn=memory_limit, capture_output=True,
                                text=True, timeout=120)
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)
    cli("init")
    relative = Path("containers") / hashlib.sha256(b'"test/large"').hexdigest() / "data/payload.bin"
    original = home / "collections/global" / relative
    original.parent.mkdir(parents=True)
    with original.open("wb") as stream:
        stream.truncate(128 * 1024 * 1024)
        stream.write(b"begin")
        stream.seek(-3, 2)
        stream.write(b"end")
    archive = home / "backup.rhyven"
    result = cli("backup", "global", "--out", str(archive))
    assert result["format"] == 3 and result["bytes"] > 128 * 1024 * 1024
    cli("restore", str(archive), "--collection", "restored")
    assert digest(original) == digest(home / "collections/restored" / relative)
    assert archive.stat().st_size < 129 * 1024 * 1024
print("PASS: 128 MiB backup/restore under a 96 MiB address-space limit; matching checksum and no JSON expansion")
