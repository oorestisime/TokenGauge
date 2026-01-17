#!/usr/bin/env bash
set -euo pipefail

REPO="${TOKENGAUGE_REPO:-oorestisime/TokenGauge}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
TMP_DIR=$(mktemp -d)

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

get_latest_tag() {
  local repo="$1"
  local api_json
  api_json=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest")

  if command -v jq >/dev/null 2>&1; then
    printf '%s' "$api_json" | jq -r '.tag_name // empty'
  else
    echo "Missing jq for JSON parsing" >&2
    return 1
  fi
}

latest=$(get_latest_tag "$REPO")
if [[ -z "$latest" ]]; then
  echo "Failed to find latest release for $REPO" >&2
  exit 1
fi

arch=$(uname -m)
case "$arch" in
  x86_64) asset_arch="x86_64" ;;
  aarch64|arm64) asset_arch="aarch64" ;;
  *) echo "Unsupported arch: $arch" >&2; exit 1 ;;
 esac

asset="tokengauge-$latest-linux-$asset_arch.tar.gz"
url="https://github.com/$REPO/releases/download/$latest/$asset"

curl -fL "$url" -o "$TMP_DIR/$asset"

tar -xzf "$TMP_DIR/$asset" -C "$TMP_DIR"

install -m 0755 "$TMP_DIR/tokengauge-waybar" "$INSTALL_DIR/tokengauge-waybar"
install -m 0755 "$TMP_DIR/tokengauge-tui" "$INSTALL_DIR/tokengauge-tui"

printf '%s\n' "Updated tokengauge to $latest in $INSTALL_DIR"
