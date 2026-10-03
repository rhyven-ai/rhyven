#!/usr/bin/env bash
# Rhyven bootstrap: no compiler or Python required.
set -euo pipefail
default_download_base_url=
embedded_public_key=
public_key=
download_base_url=${RHYVEN_DOWNLOAD_BASE_URL:-$default_download_base_url}
version=latest
source_dir=
bin_dir=${RHYVEN_BIN_DIR:-$HOME/.local/bin}
containers=0
yes=0
modify_path=1
usage() { cat <<'EOF'
Usage: bash install.sh [--containers] [--yes] [--version TAG] [--download-base-url HTTPS_URL]
                       [--from-dir RELEASE_DIRECTORY] [--bin-dir DIRECTORY] [--no-modify-path]
                       [--public-key TRUSTED_PEM_FILE]
Installs a verified prebuilt Rhyven and initializes its local home.
--containers initiates Docker setup; --yes authorizes the described system changes.
Without --containers, installs the marketplace and declarative runtime only.
Downloads prebuilt binaries directly over HTTPS; no GitHub account or compiler required.
Existing app data is preserved. --from-dir supports offline installation.
Release signatures are mandatory. Obtain any custom public key independently of the download.
EOF
}
while [ "$#" -gt 0 ]; do
  case "$1" in
    --containers) containers=1; shift ;;
    --yes) yes=1; shift ;;
    --no-modify-path) modify_path=0; shift ;;
    --version|--download-base-url|--from-dir|--bin-dir|--public-key)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      case "$1" in --version) version=$2 ;; --download-base-url) download_base_url=$2 ;; --from-dir) source_dir=$2 ;; --bin-dir) bin_dir=$2 ;; --public-key) public_key=$2 ;; esac
      shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
done
valid_version() { [[ "$1" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([-+][A-Za-z0-9.-]+)?$ ]]; }
[ "$version" = latest ] || valid_version "$version" || { echo 'Expected latest or a version such as v1.0.0' >&2; exit 2; }
if [ -z "$source_dir" ]; then
  [[ "$download_base_url" =~ ^https://[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?(:[0-9]+)?(/[A-Za-z0-9._~/-]*)?$ ]] || {
    echo 'Use the installer from the Rhyven website, --download-base-url HTTPS_URL, or --from-dir RELEASE_DIRECTORY.' >&2; exit 2;
  }
  download_base_url=${download_base_url%/}
  command -v curl >/dev/null || { echo 'curl is required for HTTPS downloads; alternatively use --from-dir.' >&2; exit 1; }
fi
case "$bin_dir" in /*) ;; *) echo '--bin-dir must be absolute' >&2; exit 2 ;; esac
case "$bin_dir" in *$'\n'*|*$'\r'*) echo 'Invalid install directory' >&2; exit 2 ;; esac
case "$(uname -s)" in Linux) platform=linux ;; Darwin) platform=macos ;; *) echo 'This bootstrap supports Linux and macOS. Native Windows is not yet supported; use WSL.' >&2; exit 2 ;; esac
case "$(uname -m)" in x86_64|amd64) arch=x86_64 ;; aarch64|arm64) arch=aarch64 ;; *) echo 'Unsupported CPU architecture' >&2; exit 2 ;; esac
asset=rhyven-$platform-$arch
work=$(mktemp -d)
staged=
cleanup() { rm -rf -- "$work"; if [ -n "$staged" ]; then rm -f -- "$staged"; fi; }
trap cleanup EXIT
command -v openssl >/dev/null || { echo 'OpenSSL is required to verify release signatures.' >&2; exit 1; }
if [ -n "$public_key" ]; then
  cp -- "$public_key" "$work/trusted-key.pem"
elif [ -n "$embedded_public_key" ]; then
  printf '%s\n' "$embedded_public_key" > "$work/trusted-key.pem"
else
  echo 'No trusted release key. Use the signed website installer or --public-key from a trusted source.' >&2; exit 1
fi
download() {
  curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL --retry 3 --connect-timeout 15 --max-time 300 "$1" -o "$2" || {
    echo 'Release download failed. Check the download URL, connection and availability for your platform.' >&2; exit 1;
  }
}
# Resolve latest once so checksums and binaries always come from one immutable release.
if [ "$version" = latest ] && [ -z "$source_dir" ]; then
  download "$download_base_url/VERSION" "$work/VERSION"
  version="v$(cat "$work/VERSION")"
  valid_version "$version" || { echo 'Invalid published VERSION' >&2; exit 1; }
fi
fetch() {
  local file=$1
  if [ -n "$source_dir" ]; then cp -- "$source_dir/$file" "$work/$file"; return; fi
  download "$download_base_url/releases/$version/$file" "$work/$file"
}
fetch SHA256SUMS
fetch SHA256SUMS.sig
openssl dgst -sha256 -verify "$work/trusted-key.pem" -signature "$work/SHA256SUMS.sig" "$work/SHA256SUMS" >/dev/null 2>&1 || {
  echo 'Release signature verification failed; existing installation unchanged' >&2; exit 1;
}
fetch "$asset"
expected=$(awk -v name="$asset" '$2 == name {print $1}' "$work/SHA256SUMS")
[[ "$expected" =~ ^[0-9a-fA-F]{64}$ ]] || { echo 'Missing or ambiguous release checksum' >&2; exit 1; }
if command -v sha256sum >/dev/null; then actual=$(sha256sum "$work/$asset"); else actual=$(shasum -a 256 "$work/$asset"); fi
actual=${actual%% *}
[ "$actual" = "$expected" ] || { echo 'Checksum mismatch; existing installation unchanged' >&2; exit 1; }
chmod 755 "$work/$asset"
# Check loader/OS compatibility before replacing the existing executable.
"$work/$asset" --version
if [[ "$version" =~ ^v[0-9]+\. ]]; then
  [ "$("$work/$asset" --version)" = "rhyven ${version#v}" ] || { echo 'Release tag and binary version disagree; existing installation unchanged' >&2; exit 1; }
fi
"$work/$asset" setup --help >/dev/null || { echo "Release predates integrated setup; select a newer installer release." >&2; exit 1; }
mkdir -p "$bin_dir"
[ ! -L "$bin_dir/rhyven" ] || { echo 'Refusing to replace an existing rhyven symlink; choose another --bin-dir.' >&2; exit 1; }
staged=$(mktemp "$bin_dir/.rhyven-install.XXXXXX")
cp "$work/$asset" "$staged"
chmod 755 "$staged"
mv -f "$staged" "$bin_dir/rhyven"
staged=
export PATH="$bin_dir:$PATH"
# Single-quote the path for shell startup files; avoid evaluating its contents.
quoted=$(printf '%s' "$bin_dir" | sed "s/'/'\\\\''/g")
path_line="export PATH='$quoted':\"\$PATH\" # Rhyven installer"
profiles=("$HOME/.profile")
case "${SHELL:-}" in
  */zsh) profiles+=("${ZDOTDIR:-$HOME}/.zshrc") ;;
  */bash)
    profiles+=("$HOME/.bashrc")
    # Bash reads only the first existing login profile; it can skip .profile.
    for login_profile in "$HOME/.bash_profile" "$HOME/.bash_login"; do
      if [ -f "$login_profile" ]; then profiles+=("$login_profile"); break; fi
    done
    ;;
  *) echo "Add $bin_dir to your shell's PATH if it does not read ~/.profile." >&2 ;;
esac
if [ "$modify_path" = 1 ]; then
for profile in "${profiles[@]}"; do
  if ! grep -Fqx "$path_line" "$profile" 2>/dev/null; then printf '\n%s\n' "$path_line" >> "$profile"; fi
done
fi
set -- "$bin_dir/rhyven" setup
if [ "$containers" = 1 ]; then set -- "$@" --containers; fi
if [ "$yes" = 1 ]; then set -- "$@" --yes; fi
# Older releases do not seed the catalog during setup. Keep this bootstrap compatible.
if [ -n "$source_dir" ]; then export RHYVEN_SETUP_OFFLINE=1; fi
setup_output=$("$@")
printf '%s\n' "$setup_output"
if ! printf '%s' "$setup_output" | grep -q '"marketplace":'; then
  if [ "${RHYVEN_SETUP_OFFLINE:-0}" = 1 ]; then
    printf '\nOffline setup: using bundled apps. Fetch the catalog later with:\n  %s/rhyven registry-sync rhyven-ai/registry --anonymous\n' "'$quoted'"
  else
    printf '\nFetching the public marketplace catalog...\n'
    if ! "$bin_dir/rhyven" registry-sync rhyven-ai/registry --anonymous; then
      printf '\nCatalog download failed; Rhyven is installed and bundled apps remain available.\n' >&2
      printf 'Retry: %s/rhyven registry-sync rhyven-ai/registry --anonymous\n' "'$quoted'" >&2
      printf 'If the error mentions unsupported fields, update Rhyven using the current installer and retry.\n' >&2
    fi
  fi
fi
printf '\nRhyven installed at %s/rhyven\n' "$bin_dir"
printf '\nOpen the terminal marketplace (TUI):\n  %s/rhyven\n' "'$quoted'"
printf '\nOr connect an agent (choose your client):\n'
printf '  %s/rhyven connect --client codex\n' "'$quoted'"
printf '  %s/rhyven connect --client claude\n' "'$quoted'"
printf '  %s/rhyven connect --client cursor\n' "'$quoted'"
printf '  %s/rhyven connect --client vscode\n' "'$quoted'"
printf '\nFor other MCP clients, print connection instructions:\n  %s/rhyven --agent\n' "'$quoted'"
printf '\nAfter connecting, reload your agent client and ask:\n'
printf '  "Use Rhyven to search the marketplace. Show me an app and its permissions before installing it."\n'
printf '\nVerify the connection:\n  %s/rhyven connect --check\n' "'$quoted'"
if [ "$modify_path" = 1 ]; then
  printf '\nTo use rhyven in this terminal now, run:\n  %s\n  rhyven\n' "$path_line"
  printf '\nOr open a new terminal, then run: rhyven\n'
  printf 'The installer cannot change PATH in the terminal that launched it.\n'
else
  printf '\nPATH was not changed. Use the full commands above or add the install directory to PATH.\n'
fi
if [ "$containers" = 1 ]; then
  printf '\nIf Docker setup reports pending, follow its instructions and resume with:\n  %s/rhyven setup --containers\n' "'$quoted'"
fi
