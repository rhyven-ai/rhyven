#!/usr/bin/env bash
# Package an already-built native executable; run on each release platform.
set -euo pipefail
binary=${1:-target/release/rhyven}
output=${2:-dist}
case "$(uname -s)" in Linux) platform=linux ;; Darwin) platform=macos ;; *) echo 'Unsupported release OS' >&2; exit 1 ;; esac
case "$(uname -m)" in x86_64|amd64) arch=x86_64 ;; arm64|aarch64) arch=aarch64 ;; *) exit 1 ;; esac
mkdir -p "$output"
asset=rhyven-$platform-$arch
cp "$binary" "$output/$asset"
chmod 755 "$output/$asset"
cp LICENSE NOTICE "$output/"
"$binary" license --third-party | python3 -c 'import json,sys; print(json.load(sys.stdin)["third_party_notices"], end="")' > "$output/THIRD_PARTY_NOTICES.txt"
version=$("$binary" --version)
version=${version#rhyven }
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][A-Za-z0-9.-]+)?$ ]] || { echo 'Invalid runtime version' >&2; exit 1; }
printf '%s\n' "$version" > "$output/VERSION"
# Pin this bootstrap to its own release; never silently select an older stable release.
sed "s/^version=latest$/version=v$version/" scripts/install.sh > "$output/install.sh"
(cd "$output"; if command -v sha256sum >/dev/null; then sha256sum "$asset" install.sh VERSION LICENSE NOTICE THIRD_PARTY_NOTICES.txt; else shasum -a 256 "$asset" install.sh VERSION LICENSE NOTICE THIRD_PARTY_NOTICES.txt; fi) > "$output/SHA256SUMS"
