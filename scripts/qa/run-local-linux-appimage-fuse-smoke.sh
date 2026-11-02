#!/usr/bin/env bash
# Exact locally built AppImage, kernel FUSE mount and real isolated Xvfb GUI.
# Does not use APPIMAGE_EXTRACT_AND_RUN; relies on the existing safe GUI harness.
set -Eeuo pipefail
umask 077
bundle=/mnt/d/coding/codex/constellation-ux-rebuild/target-linux/release/bundle/appimage/Constellation_0.1.2_amd64.AppImage
source_root=/home/saba/constellation-ux-linux
[[ -s "$bundle" && -f "$source_root/apps/desktop/src-tauri/tauri.conf.json" ]] || exit 2
[[ -c /dev/fuse ]] || { echo 'FUSE character device unavailable' >&2; exit 3; }
run="/home/saba/.cache/constellation-linux-packages/run.ux-fuse-$(date +%Y%m%d%H%M%S)-$$"
mkdir -p "$run/output" "$run/source/apps/desktop/src-tauri"
cp "$source_root/apps/desktop/src-tauri/tauri.conf.json" "$run/source/apps/desktop/src-tauri/"
copy="$run/output/Constellation_0.1.2_x86_64.AppImage"
cp "$bundle" "$copy"
cmp -s "$bundle" "$copy"
chmod 0755 "$copy"
echo "QA_FUSE_SOURCE_SHA256=$(sha256sum "$bundle")"
echo "QA_FUSE_RUN=$run"
bash "$source_root/tests/linux-gui-xvfb.sh" "$run" appimage-fuse 12
