#!/usr/bin/env bash
# Builds a release binary and installs it for the current user: the binary as
# ~/.local/bin/tailscale-desktop, plus the desktop entry and the icons.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release
data=${XDG_DATA_HOME:-$HOME/.local/share}
install -Dm755 target/release/tailscale-desktop-unofficial "$HOME/.local/bin/tailscale-desktop"
install -Dm644 packaging/tailscale-desktop-unofficial.desktop "$data/applications/tailscale-desktop-unofficial.desktop"
install -Dm644 assets/icon.svg "$data/icons/hicolor/scalable/apps/tailscale-desktop-unofficial.svg"
install -Dm644 assets/icon-256.png "$data/icons/hicolor/256x256/apps/tailscale-desktop-unofficial.png"
gtk-update-icon-cache -q "$data/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$data/applications" 2>/dev/null || true
echo "Installed Tailscale Desktop (unofficial)"
