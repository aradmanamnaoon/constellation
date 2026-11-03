#!/usr/bin/env bash
# Test the exact locally built x86_64 AppImage, extracted in a private Xvfb session.
set -Eeuo pipefail
umask 077
bundle=/mnt/d/coding/codex/constellation-ux-rebuild/target-linux/release/bundle/appimage/Constellation_0.1.2_amd64.AppImage
source_root=/home/saba/constellation-ux-linux
[[ -f "$bundle" && -f "$source_root/apps/desktop/src-tauri/tauri.conf.json" ]] || exit 2
run="/home/saba/.cache/constellation-linux-packages/run.ux-appimage-$(date +%Y%m%d%H%M%S)-$$"
mkdir -p "$run/output" "$run/source/apps/desktop/src-tauri"
cp "$source_root/apps/desktop/src-tauri/tauri.conf.json" "$run/source/apps/desktop/src-tauri/"
copy="$run/output/Constellation_0.1.2_x86_64.AppImage"
cp "$bundle" "$copy"
cmp -s "$bundle" "$copy" || { echo 'AppImage copy differs from original' >&2; exit 3; }
chmod +x "$copy"
echo "QA_APPIMAGE_PACKAGE=$(sha256sum "$bundle")"
echo "QA_APPIMAGE_RUN=$run"
bash "$source_root/tests/linux-gui-xvfb.sh" "$run" appimage 8
