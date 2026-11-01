#!/usr/bin/env bash
# Extract a locally built Constellation .deb into a disposable private directory
# and exercise its real Linux window under Xvfb without touching the host desktop.
set -Eeuo pipefail
umask 077

source_root="${1:-/home/saba/constellation-ux-linux}"
bundle="${2:-/mnt/d/coding/codex/constellation-ux-rebuild/target-linux/release/bundle/deb/Constellation_0.1.2_amd64.deb}"
case "$source_root" in
  /home/saba/constellation-ux-linux) ;;
  *) echo 'Only the isolated Constellation Linux source tree is allowed' >&2; exit 2 ;;
esac
[[ -f "$bundle" ]] || { echo 'No Linux package at requested path' >&2; exit 2; }
[[ -f "$source_root/apps/desktop/src-tauri/tauri.conf.json" ]] || exit 2
run="/home/saba/.cache/constellation-linux-packages/run.ux-$(date +%Y%m%d%H%M%S)-$$"
mkdir -p "$run/source/apps/desktop/src-tauri" "$run/extracted"
cp "$source_root/apps/desktop/src-tauri/tauri.conf.json" "$run/source/apps/desktop/src-tauri/"
dpkg-deb -x "$bundle" "$run/extracted"
echo "QA_LINUX_PACKAGE=$(sha256sum "$bundle")"
echo "QA_LINUX_RUN=$run"
bash "$source_root/tests/linux-gui-xvfb.sh" "$run" deb 5
