#!/usr/bin/env bash
set -euo pipefail

REPO="steipete/CodexBar"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
TMP_DIR=$(mktemp -d)

if [[ -t 1 ]]; then
  COLOR_RESET="\033[0m"
  COLOR_GREEN="\033[0;32m"
  COLOR_YELLOW="\033[0;33m"
  COLOR_BLUE="\033[0;34m"
  COLOR_RED="\033[0;31m"
else
  COLOR_RESET=""
  COLOR_GREEN=""
  COLOR_YELLOW=""
  COLOR_BLUE=""
  COLOR_RED=""
fi

info() {
  printf '%b\n' "${COLOR_BLUE}$*${COLOR_RESET}"
}

success() {
  printf '%b\n' "${COLOR_GREEN}$*${COLOR_RESET}"
}

warn() {
  printf '%b\n' "${COLOR_YELLOW}$*${COLOR_RESET}"
}

fail() {
  printf '%b\n' "${COLOR_RED}$*${COLOR_RESET}" >&2
}

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

get_latest_tag() {
  local repo="$1"
  local api_json
  info "Fetching latest release for $repo"
  api_json=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest")

  if command -v jq >/dev/null 2>&1; then
    printf '%s' "$api_json" | jq -r '.tag_name // empty'
  else
    fail "Missing jq for JSON parsing"
    return 1
  fi
}

latest=$(get_latest_tag "$REPO" | tail -n 1)
if [[ -z "$latest" ]]; then
  fail "Failed to find latest release for $REPO"
  exit 1
fi

arch=$(uname -m)
case "$arch" in
  x86_64) asset_arch="x86_64" ;;
  aarch64|arm64) asset_arch="aarch64" ;;
  *) fail "Unsupported arch: $arch"; exit 1 ;;
 esac

asset="CodexBarCLI-$latest-linux-$asset_arch.tar.gz"
url="https://github.com/$REPO/releases/download/$latest/$asset"

info "Downloading CodexBarCLI $latest"
curl -fL "$url" -o "$TMP_DIR/$asset"

tar -xzf "$TMP_DIR/$asset" -C "$TMP_DIR"

if [[ -f "$TMP_DIR/CodexBarCLI" ]]; then
  install -m 0755 "$TMP_DIR/CodexBarCLI" "$INSTALL_DIR/CodexBarCLI"
fi
if [[ -f "$TMP_DIR/codexbar" ]]; then
  install -m 0755 "$TMP_DIR/codexbar" "$INSTALL_DIR/codexbar"
else
  ln -sf "$INSTALL_DIR/CodexBarCLI" "$INSTALL_DIR/codexbar"
fi

success "Installed CodexBarCLI $latest to $INSTALL_DIR"
