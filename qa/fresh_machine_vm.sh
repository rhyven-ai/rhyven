#!/usr/bin/env bash
# Disposable Ubuntu VM: real dependency installation and actual guest OS reboot.
# Host prerequisites: qemu-system-x86, qemu-utils, cloud-image-utils, curl, ssh.
# SSH commands intentionally expand tilde and variables inside the guest shell.
# shellcheck disable=SC2029,SC2088,SC2016
set -euo pipefail
release=$(realpath "${1:?release directory required (VERSION is required for a public download)}")
source_root=$(realpath "${2:-.}")
download_url=${3:-}
if [ -n "$download_url" ]; then
  [[ "$download_url" =~ ^https://[A-Za-z0-9][A-Za-z0-9.-]*(/[A-Za-z0-9._~/-]*)?$ ]] || { echo 'Expected an HTTPS download origin' >&2; exit 2; }
fi
expected_version=$(cat "$release/VERSION")
[[ "$expected_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][A-Za-z0-9.-]+)?$ ]] || exit 2
work=$(mktemp -d)
vm_pid=
cleanup() {
  if [ -n "$vm_pid" ]; then kill "$vm_pid" 2>/dev/null || true; wait "$vm_pid" 2>/dev/null || true; fi
  if [ -n "${RHYVEN_VM_REPORT_DIR:-}" ]; then
    mkdir -p "$RHYVEN_VM_REPORT_DIR"
    for name in console.log setup.json unavailable.json stopped.json boot-before boot-after tui.json; do
      if [ -f "$work/$name" ]; then cp "$work/$name" "$RHYVEN_VM_REPORT_DIR/$name"; fi
    done
  fi
  rm -rf "$work"
}
trap cleanup EXIT
ssh-keygen -q -t ed25519 -N '' -f "$work/key"
base=https://cloud-images.ubuntu.com/releases/noble/release
asset=ubuntu-24.04-server-cloudimg-amd64.img
curl -fsSL --retry 3 "$base/$asset" -o "$work/$asset"
curl -fsSL --retry 3 "$base/SHA256SUMS" -o "$work/image-checksums"
(cd "$work"; awk -v file="$asset" '$2 == "*"file || $2 == file' image-checksums > image.sha256; test -s image.sha256; sha256sum -c image.sha256)
qemu-img create -f qcow2 -F qcow2 -b "$work/$asset" "$work/disk.qcow2" 18G
cat > "$work/user-data" <<EOF
#cloud-config
users:
  - name: tester
    groups: [sudo]
    shell: /bin/bash
    sudo: ALL=(ALL) NOPASSWD:ALL
    ssh_authorized_keys:
      - $(cat "$work/key.pub")
ssh_pwauth: false
EOF
printf 'instance-id: rhyven-acceptance\nlocal-hostname: rhyven-acceptance\n' > "$work/meta-data"
cloud-localds "$work/seed.img" "$work/user-data" "$work/meta-data"
port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
accel=tcg
if [ -r /dev/kvm ] && [ -w /dev/kvm ]; then accel=kvm; fi
qemu-system-x86_64 -accel "$accel" -m 3072 -smp 2 -nographic \
  -drive "file=$work/disk.qcow2,format=qcow2" -drive "file=$work/seed.img,format=raw" \
  -nic "user,hostfwd=tcp:127.0.0.1:$port-:22" > "$work/console.log" 2>&1 &
vm_pid=$!
ssh_options=(-i "$work/key" -p "$port" -o BatchMode=yes -o ConnectTimeout=5 -o StrictHostKeyChecking=accept-new -o "UserKnownHostsFile=$work/known-hosts")
guest() { ssh "${ssh_options[@]}" tester@127.0.0.1 "$@"; }
wait_guest() {
  for _ in $(seq 1 180); do
    if guest true 2>/dev/null; then return; fi
    if ! kill -0 "$vm_pid" 2>/dev/null; then tail -60 "$work/console.log"; return 1; fi
    sleep 2
  done
  tail -60 "$work/console.log"; return 1
}
wait_guest
guest 'sudo cloud-init status --wait'
tar -C "$source_root" -cf - examples/container-service-python qa/public_registry_check.py qa/universal_market_check.py qa/public_container_check.py qa/tui_refresh_check.py | guest 'mkdir source; tar -xf - -C source'
guest 'set -e; test ! -e /var/run/docker.sock; ! command -v docker; ! command -v rhyven; ! command -v cargo; ! command -v gh'
if [ -n "$download_url" ]; then
  guest "bash -o pipefail -c 'curl -fsSL $download_url/install.sh | bash -s -- --containers --yes'"
else
  tar -C "$release" -cf - install.sh SHA256SUMS SHA256SUMS.sig release-key.pem VERSION LICENSE NOTICE THIRD_PARTY_NOTICES.txt rhyven-linux-x86_64 | guest 'mkdir release; tar -xf - -C release'
  guest 'bash release/install.sh --from-dir release --containers --yes --no-modify-path'
fi
guest "test \"\$(~/.local/bin/rhyven --version)\" = 'rhyven $expected_version'"
guest "~/.local/bin/rhyven --agent; ~/.local/bin/rhyven connect --client generic --print; ~/.local/bin/rhyven connect --check"
# Refresh the systemd user manager as well as login-shell supplementary groups.
guest 'sudo reboot' || true
sleep 3
wait_guest
guest '~/.local/bin/rhyven setup --containers; ~/.local/bin/rhyven doctor' > "$work/setup.json"
if [ -n "$download_url" ]; then
  guest "bash -lc 'command -v rhyven; rhyven --version'"
  guest 'python3 source/qa/public_registry_check.py ~/.local/bin/rhyven'
  guest 'sudo apt-get update -qq && sudo apt-get install -y python3-pyte'
  guest 'python3 source/qa/tui_refresh_check.py ~/.local/bin/rhyven tui.json'
  guest 'cat tui.json' > "$work/tui.json"
  guest 'python3 source/qa/public_container_check.py ~/.local/bin/rhyven'
fi
guest 'docker info >/dev/null; docker build --iidfile image.id source/examples/container-service-python'
guest "python3 - <<'PY'
import json
from pathlib import Path
p=json.loads(Path('source/examples/container-service-python/app.json').read_text())
p['execution']['image']=Path('image.id').read_text().strip()
Path('service.json').write_text(json.dumps(p))
PY"
guest 'set -e; ~/.local/bin/rhyven install service.json --accept-permissions; mkdir -p ~/.config/systemd/user; ~/.local/bin/rhyven daemon unit --out ~/.config/systemd/user/rhyven-acceptance.service; sudo loginctl enable-linger tester; systemctl --user daemon-reload; systemctl --user enable --now rhyven-acceptance; for i in $(seq 1 100); do test -S ~/.rhyven/supervisor/control.sock && break; sleep .1; done; ~/.local/bin/rhyven service start example/background-counter'
guest 'cat /proc/sys/kernel/random/boot_id' > "$work/boot-before"
guest 'sudo reboot' || true
sleep 3
wait_guest
guest 'cat /proc/sys/kernel/random/boot_id' > "$work/boot-after"
if cmp -s "$work/boot-before" "$work/boot-after"; then echo 'Guest did not reboot' >&2; exit 1; fi
guest "python3 - <<'PY'
import json,subprocess,time
from pathlib import Path
binary=str(Path.home()/'.local/bin/rhyven')
for attempt in range(60):
 p=subprocess.run([binary,'service','status','example/background-counter'],capture_output=True,text=True)
 if not p.returncode and json.loads(p.stdout).get('state')=='ready': break
 time.sleep(1)
else: raise AssertionError(p.stdout+p.stderr)
result=subprocess.check_output([binary,'call','rhyven_call',json.dumps({'category':'example/background-counter','function':'action_status','args':{}})],text=True)
assert 'ticks' in json.loads(result),result
print('PASS: enabled service recovered after actual guest reboot')
PY"
guest 'sudo systemctl stop docker.service docker.socket; ~/.local/bin/rhyven doctor' > "$work/unavailable.json"
python3 - "$work/unavailable.json" <<'PYTEST'
import json,sys
assert json.load(open(sys.argv[1]))['container']['ready'] is False
PYTEST
guest 'sudo systemctl start docker'
guest "python3 - <<'PY'
import json,subprocess,time
from pathlib import Path
binary=str(Path.home()/'.local/bin/rhyven')
for attempt in range(60):
 p=subprocess.run([binary,'service','status','example/background-counter'],capture_output=True,text=True)
 if not p.returncode and json.loads(p.stdout).get('state')=='ready': break
 time.sleep(1)
else: raise AssertionError(p.stdout+p.stderr)
result=subprocess.check_output([binary,'call','rhyven_call',json.dumps({'category':'example/background-counter','function':'action_status','args':{}})],text=True)
assert 'ticks' in json.loads(result),result
print('PASS: enabled service recovered automatically after Docker returned')
PY"
guest '~/.local/bin/rhyven service stop example/background-counter; sudo reboot' || true
sleep 3
wait_guest
guest "~/.local/bin/rhyven service status example/background-counter" > "$work/stopped.json"
python3 - "$work/stopped.json" <<'PY'
import json,sys
status=json.load(open(sys.argv[1]))
assert status['state']=='stopped' and status['desired']=='stopped' and status['explicitly_stopped'] is True,status
PY
guest 'docker ps --filter label=rhyven.service --format "{{.Names}}"' > "$work/stopped-containers"
test ! -s "$work/stopped-containers"
echo 'PASS: Ubuntu 24.04 fresh dependency install, Docker access after user-manager refresh/reboot, systemd startup, actual OS reboot, unavailable Docker and repair, stopped intent across reboot'
