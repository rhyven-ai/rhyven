#!/usr/bin/env bash
# Embedded in rhyven setup. stdout is redirected to stderr by the CLI.
set -euo pipefail
pending() { echo "$*; rerun rhyven setup --containers after resolving this." >&2; exit 20; }
approve() {
  echo "$1" >&2
  if [ "${RHYVEN_SETUP_YES:-0}" = 1 ]; then return; fi
  local answer
  if ! { read -r -p 'Proceed? [y/N] ' answer </dev/tty; } 2>/dev/null; then
    pending 'Approval required: use an interactive terminal or explicitly pass --yes'
  fi
  case "$answer" in y|Y|yes|YES) ;; *) pending 'Dependency setup declined; declarative apps remain available' ;; esac
}
[ "$(id -u)" != 0 ] || pending 'Run Rhyven as your normal user; setup invokes sudo only for system changes'
case "$(uname -s)" in
Linux)
  if ! command -v systemctl >/dev/null || [ ! -d /run/systemd/system ]; then
    pending 'Enable systemd before automatic Docker setup (on WSL, enable systemd and restart WSL)'
  fi
  if command -v docker >/dev/null; then
    if docker info >/dev/null 2>&1; then pending 'Existing engine does not meet Rhyven requirements; inspect rhyven doctor'; fi
    # Never switch an existing context, remote endpoint, or Docker Desktop installation.
    context=$(docker context show 2>/dev/null || true)
    if [ -n "${DOCKER_HOST:-}" ] || [ -n "${DOCKER_CONTEXT:-}" ]; then
      pending 'An explicit Docker endpoint/context is selected; start or repair that engine'
    fi
    case "$context" in
      rootless)
        approve 'Start the existing rootless Docker user service?'
        systemctl --user start docker
        exit 0 ;;
      default)
        approve 'Start the existing system Docker service?'
        sudo systemctl start docker
        if docker info >/dev/null 2>&1; then exit 0; fi
        if id -nG | tr ' ' '\n' | grep -qx docker; then pending 'Docker remains unavailable; inspect systemctl status docker'; fi
        approve 'Add your account to the docker group? This grants root-equivalent Docker access and requires a new login.'
        sudo usermod -aG docker "$(id -un)"
        pending 'Log out of all sessions or reboot to refresh Docker group membership, including the systemd user manager' ;;
      *) pending 'Start your selected Docker engine; setup will not replace it' ;;
    esac
  fi
  # /etc/os-release is system-owned, not app package input.
  # shellcheck source=/dev/null
  . /etc/os-release
  case "${ID:-}" in ubuntu|debian) ;; *) pending 'Automatic engine installation currently supports Ubuntu and Debian; install a compatible Docker engine on this distribution' ;; esac
  distro=$ID
  codename=${UBUNTU_CODENAME:-${VERSION_CODENAME:-}}
  [[ "$codename" =~ ^[a-z]+$ ]] || pending 'Distribution codename unavailable'
  for package in docker.io docker-compose docker-compose-v2 docker-doc podman-docker containerd runc; do
    if dpkg-query -W -f='${Status}' "$package" 2>/dev/null | grep -q 'install ok installed'; then
      pending "Existing $package package may conflict with Docker CE; setup will not remove it automatically"
    fi
  done
  approve 'Install Docker Engine from the signed Docker apt repository, start its system service, and grant your account root-equivalent docker-group access?'
  work=$(mktemp -d)
  trap 'rm -rf -- "$work"' EXIT
  sudo apt-get update
  sudo apt-get install -y ca-certificates curl
  curl --proto '=https' --tlsv1.2 -fsSL "https://download.docker.com/linux/$distro/gpg" -o "$work/docker.asc"
  sudo install -m 0755 -d /etc/apt/keyrings
  sudo install -m 0644 "$work/docker.asc" /etc/apt/keyrings/rhyven-docker.asc
  arch=$(dpkg --print-architecture)
  printf 'Types: deb\nURIs: https://download.docker.com/linux/%s\nSuites: %s\nComponents: stable\nArchitectures: %s\nSigned-By: /etc/apt/keyrings/rhyven-docker.asc\n' "$distro" "$codename" "$arch" > "$work/docker.sources"
  if [ -e /etc/apt/sources.list.d/docker.sources ] || [ -e /etc/apt/sources.list.d/docker.list ]; then
    pending 'An existing Docker apt source needs review; setup will not overwrite it'
  fi
  sudo install -m 0644 "$work/docker.sources" /etc/apt/sources.list.d/rhyven-docker.sources
  sudo apt-get update
  sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
  sudo systemctl enable --now docker
  sudo usermod -aG docker "$(id -un)"
  pending 'Docker installed; log out of all sessions or reboot to refresh docker-group access, including systemd user services'
  ;;
Darwin)
  if command -v docker >/dev/null && docker info >/dev/null 2>&1; then
    pending 'Existing engine does not meet Rhyven requirements; inspect rhyven doctor'
  fi
  if [ ! -d /Applications/Docker.app ]; then
    if command -v docker >/dev/null; then pending 'Existing Docker CLI detected; start or repair its engine instead of replacing it'; fi
    approve 'Download and install Docker Desktop from Docker? macOS may ask for administrator access; Docker terms must be accepted in its UI.'
    case "$(uname -m)" in arm64) arch=arm64 ;; x86_64) arch=amd64 ;; *) pending 'Unsupported Mac architecture' ;; esac
    work=$(mktemp -d)
    mounted=0
    cleanup() { if [ "$mounted" = 1 ]; then hdiutil detach "$work/mount" >/dev/null || true; fi; rm -rf -- "$work"; }
    trap cleanup EXIT
    curl --proto '=https' --tlsv1.2 -fL --retry 3 "https://desktop.docker.com/mac/main/$arch/Docker.dmg" -o "$work/Docker.dmg"
    mkdir "$work/mount"
    hdiutil attach -nobrowse -mountpoint "$work/mount" "$work/Docker.dmg"
    mounted=1
    # Verify vendor identity before invoking the installer with administrator privileges.
    codesign --verify --deep --strict -R 'anchor apple generic and certificate leaf[subject.OU] = "9BNSXJN65R"' "$work/mount/Docker.app"
    spctl --assess --type execute "$work/mount/Docker.app"
    sudo "$work/mount/Docker.app/Contents/MacOS/install" --user="$(id -un)"
  else
    approve 'Start the installed Docker Desktop application?'
  fi
  open -a Docker
  pending 'Finish Docker Desktop onboarding, wait for the engine to start, and resume setup'
  ;;
*) pending 'Automatic container setup is not available on this operating system' ;;
esac
