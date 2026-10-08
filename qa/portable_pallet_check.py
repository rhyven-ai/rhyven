"""Local portable-library CLI acceptance; only reviewed fixtures and temporary state."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1]).resolve())
repository = Path(__file__).resolve().parents[1]

with tempfile.TemporaryDirectory(prefix="rhyven-pallet-check-") as temporary:
    root = Path(temporary)
    base = [binary, "--home", str(root / "home"), "--collection", "test"]

    def cli(*args):
        return json.loads(subprocess.check_output(base + list(args), text=True))

    before = cli("call", "rhyven_categories", "{}")
    source = repository / "examples/pallet-text"
    cli("pallet", "validate", str(source))
    assert cli("pallet", "test", str(source), "--allow-host")["passed"]
    assert not cli("pallet", "save", str(source))["installed_app"]
    assert cli("call", "rhyven_categories", "{}") == before

    first = cli("pallet", "describe", "example/text-kit@0.1.0", "--export", "prepare_document")
    cached = cli("pallet", "describe", "example/text-kit@0.1.0", "--if-hash", first["contract_hash"])
    assert cached["unchanged"] and "exports" not in cached
    assert "files" not in first
    for _ in range(3):
        result = cli("pallet", "run", "example/text-kit@0.1.0", "prepare_document",
                     "--args", '{"title":" Release  Notes! "}', "--allow-host")
        assert result == {"title": "Release Notes!", "slug": "release-notes"}

    exported = root / "standalone"
    cli("pallet", "export", "example/text-kit@0.1.0", "--dir", str(exported))
    # Normal import: no Rhyven executable, protocol adapter or runtime API call.
    subprocess.run([sys.executable, "-c",
                    "from textkit import prepare_document; "
                    "assert prepare_document({'title':' Build  Once '})['slug']=='build-once'"],
                   cwd=exported, check=True)

    scaffold = root / "new-library"
    cli("pallet", "init", "local/helpers", "--dir", str(scaffold))
    assert cli("pallet", "test", str(scaffold), "--allow-host")["passed"]

    app = root / "complete-app.json"
    cli("app", "bundle", str(repository / "examples/document-intake"),
        "--pallet", "text=example/text-kit@0.1.0", "--out", str(app))
    assert cli("app", "validate", str(app))["valid"]
    assert cli("app", "test", str(app), "--allow-host")["passed"]
    # Bundling/testing also leaves the actual app collection untouched.
    assert cli("call", "rhyven_categories", "{}") == before

print("PASS: portable source, cached contracts, repeated calls, ordinary imports, "
      "scaffolding and complete-app bundling without library app installation")
