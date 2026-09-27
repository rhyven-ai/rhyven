#!/usr/bin/env bash
# Build-host release gate; scans the exact local image without publishing it.
set -euo pipefail
image=${1:?local image required}
app=${2:?app name required}
output=${3:?report directory required}
mkdir -p "$output"
scanner=$(python3 - <<'PY'
import importlib.util
from pathlib import Path
spec = importlib.util.spec_from_file_location('security', 'scripts/security-scan.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
print(module.install_tools(Path('/tmp/rhyven-security-tools'))['trivy'])
PY
)
"$scanner" image --image-src docker --scanners vuln,secret --format json --output "$output/image.json" "$image"
if [ "$app" = repo-documentation-tool ]; then
  docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges --entrypoint python3 "$image" -c '
import json, pathlib, subprocess
version = subprocess.check_output(["dpkg-query", "-W", "-f=${Version}", "linux-libc-dev"], text=True)
paths = subprocess.check_output(["dpkg-query", "-L", "linux-libc-dev"], text=True).splitlines()
files = [p for p in paths if pathlib.Path(p).is_file()]
header_only = bool(files) and all((p.startswith("/usr/include/") and p.endswith(".h")) or p.startswith("/usr/share/doc/linux-libc-dev/") for p in files)
print(json.dumps({"app":"repo-documentation-tool", "version":version, "header_only":header_only, "owned_files":files}))
' > "$output/header-inventory.json"
  python3 - "$output/header-inventory.json" "$image" <<'PY'
import json, subprocess, sys
from pathlib import Path
path = Path(sys.argv[1])
data = json.loads(path.read_text())
data['image_id'] = subprocess.check_output(['docker', 'image', 'inspect', '--format', '{{.Id}}', sys.argv[2]], text=True).strip()
path.write_text(json.dumps(data, indent=2))
PY
  python3 scripts/review-image-scan.py "$output/image.json" --inventory "$output/header-inventory.json" --out "$output/review.json"
else
  python3 scripts/review-image-scan.py "$output/image.json" --out "$output/review.json"
fi
