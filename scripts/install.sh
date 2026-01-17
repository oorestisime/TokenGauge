#!/usr/bin/env bash
set -euo pipefail

REPO="${TOKENGAUGE_REPO:-oorestisime/tokengauge}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/tokengauge"
CONFIG_FILE="$CONFIG_DIR/config.toml"
WAYBAR_CONFIG="$HOME/.config/waybar/config.jsonc"
TMP_DIR=$(mktemp -d)

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

mkdir -p "$INSTALL_DIR" "$CONFIG_DIR"

get_latest_tag() {
  local repo="$1"
  local api_json
  api_json=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest")

  if command -v python3 >/dev/null 2>&1; then
    python3 - <<'PY' <<<"$api_json"
import json
import sys
try:
    data = json.load(sys.stdin)
    print(data.get("tag_name", ""))
except Exception:
    print("")
PY
  elif command -v jq >/dev/null 2>&1; then
    printf '%s' "$api_json" | jq -r '.tag_name // empty'
  else
    echo "Missing python3 or jq for JSON parsing" >&2
    return 1
  fi
}

arch=$(uname -m)
case "$arch" in
  x86_64) asset_arch="x86_64" ;;
  aarch64|arm64) asset_arch="aarch64" ;;
  *) echo "Unsupported arch: $arch" >&2; exit 1 ;;
esac

latest=$(get_latest_tag "$REPO")
if [[ -z "$latest" ]]; then
  echo "Failed to find latest release for $REPO" >&2
  exit 1
fi

asset="tokengauge-$latest-linux-$asset_arch.tar.gz"
url="https://github.com/$REPO/releases/download/$latest/$asset"

curl -fL "$url" -o "$TMP_DIR/$asset"

tar -xzf "$TMP_DIR/$asset" -C "$TMP_DIR"

install -m 0755 "$TMP_DIR/tokengauge-waybar" "$INSTALL_DIR/tokengauge-waybar"
install -m 0755 "$TMP_DIR/tokengauge-tui" "$INSTALL_DIR/tokengauge-tui"

if [[ ! -f "$CONFIG_FILE" ]]; then
  cat <<'TOML' > "$CONFIG_FILE"
# TokenGauge configuration
codexbar_bin = "codexbar"
source = "oauth"
refresh_secs = 600
cache_file = "/tmp/tokengauge-usage.json"

[providers]
codex = true
claude = true
TOML
fi

install_codexbar() {
  local codex_repo="steipete/CodexBar"
  local codex_latest
  codex_latest=$(get_latest_tag "$codex_repo")
  if [[ -z "$codex_latest" ]]; then
    echo "Failed to find latest release for $codex_repo" >&2
    return 1
  fi

  local codex_asset="CodexBarCLI-$codex_latest-linux-$asset_arch.tar.gz"
  local codex_url="https://github.com/$codex_repo/releases/download/$codex_latest/$codex_asset"
  local codex_tmp="$TMP_DIR/codexbar"

  mkdir -p "$codex_tmp"
  curl -fL "$codex_url" -o "$codex_tmp/$codex_asset"
  tar -xzf "$codex_tmp/$codex_asset" -C "$codex_tmp"

  if [[ -f "$codex_tmp/CodexBarCLI" ]]; then
    install -m 0755 "$codex_tmp/CodexBarCLI" "$INSTALL_DIR/CodexBarCLI"
  fi
  if [[ -f "$codex_tmp/codexbar" ]]; then
    install -m 0755 "$codex_tmp/codexbar" "$INSTALL_DIR/codexbar"
  else
    ln -sf "$INSTALL_DIR/CodexBarCLI" "$INSTALL_DIR/codexbar"
  fi

  printf '%s\n' "Installed CodexBarCLI $codex_latest to $INSTALL_DIR"
}

if ! command -v codexbar >/dev/null 2>&1; then
  echo "CodexBar CLI not found. Installing..."
  if ! install_codexbar; then
    echo "Warning: failed to install CodexBar CLI. Install manually from https://github.com/steipete/CodexBar/releases" >&2
  fi
fi

if [[ -f "$WAYBAR_CONFIG" ]]; then
  backup="$WAYBAR_CONFIG.bak.tokengauge.$(date +%s)"
  cp "$WAYBAR_CONFIG" "$backup"
  if ! grep -q 'custom/tokengauge' "$WAYBAR_CONFIG"; then
    sed -i 's/"group\/tray-expander"/"custom\/tokengauge", "group\/tray-expander"/' "$WAYBAR_CONFIG"
    cat <<'JSON' >> "$WAYBAR_CONFIG"

  ,"custom/tokengauge": {
    "exec": "tokengauge-waybar",
    "return-type": "json",
    "interval": 60,
    "on-click": "omarchy-launch-or-focus-tui tokengauge-tui"
  }
JSON
  fi
fi

echo "Installed tokengauge to $INSTALL_DIR. Restart Waybar: omarchy-restart-waybar"
