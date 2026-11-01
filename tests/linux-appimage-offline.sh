#!/usr/bin/env bash
# Bundle an already verified Linux native .deb executable into an actual
# AppImage, without modifying the repository or installing host packages.
set -Eeuo pipefail
umask 077

run="${1:-}"
[[ "$run" == /home/saba/.cache/constellation-linux-packages/run.* ]] || {
  echo 'Supply the Linux packaging run directory under the approved cache' >&2; exit 2;
}
[[ -d "$run" ]] || exit 2
for tool in cmp file sha256sum stat; do command -v "$tool" >/dev/null; done

bundle_tool=/home/saba/.cache/constellation-appimage-tools/appimagetool-x86_64.AppImage
expected_tool_sha=793fca0ce4bcc309dc65d3f66669f58e3755643e4dadbe03932e7b436128494f
runtime=/home/saba/.cache/constellation-appimage-tools/runtime-x86_64
expected_runtime_sha=204acc190df96da3793525ea5ef4eccef3618aea6016682982f4e02ee290ca7f
[[ -x "$bundle_tool" ]] || { echo 'Official AppImage bundler not cached' >&2; exit 2; }
echo "$expected_tool_sha  $bundle_tool" | sha256sum -c --status || {
  echo 'AppImage bundler SHA256 differs from the inspected tool' >&2; exit 2;
}
[[ -f "$runtime" ]] || { echo 'Pinned AppImage type2 runtime not cached' >&2; exit 2; }
echo "$expected_runtime_sha  $runtime" | sha256sum -c --status || {
  echo 'AppImage type2 runtime SHA256 differs from the inspected runtime' >&2; exit 2;
}

binary="$run/extracted/usr/bin/constellation-desktop"
deb="$run/output/constellation-studio_0.1.0_amd64.deb"
icon="$run/extracted/usr/share/icons/hicolor/128x128/apps/constellation.png"
[[ -x "$binary" && -f "$deb" && -f "$icon" ]] || exit 2
reference="${CONSTELLATION_LINUX_REFERENCE_BINARY:-/home/saba/.cache/constellation-linux-desktop/release/constellation-desktop}"
[[ "$reference" == /home/saba/.cache/constellation-linux-desktop/release/constellation-desktop ||
   "$reference" == /home/saba/.cache/constellation-linux-desktop-*/release/constellation-desktop ]] || {
  echo 'Reference executable path outside dedicated Linux cache' >&2; exit 2;
}
cmp "$binary" "$reference"

appdir="$run/appimage-stage/Constellation.AppDir"
output="$run/output/Constellation_0.1.0_x86_64.AppImage"
inspect="$run/appimage-inspection"
[[ ! -e "$appdir" && ! -e "$output" && ! -e "$inspect" ]] || {
  echo 'AppImage staging/output already exists; refusing to overwrite' >&2; exit 2;
}
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" \
  "$appdir/usr/share/icons/hicolor/128x128/apps" "$inspect"
chmod 0755 "$run/appimage-stage" "$appdir" "$appdir/usr" \
  "$appdir/usr/bin" "$appdir/usr/share" \
  "$appdir/usr/share/applications" "$appdir/usr/share/icons" \
  "$appdir/usr/share/icons/hicolor" "$appdir/usr/share/icons/hicolor/128x128" \
  "$appdir/usr/share/icons/hicolor/128x128/apps"
install -m755 "$binary" "$appdir/usr/bin/constellation-desktop"
install -m644 "$icon" "$appdir/constellation.png"
install -m644 "$icon" "$appdir/usr/share/icons/hicolor/128x128/apps/constellation.png"
cat > "$appdir/app.constellation.studio.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Constellation
Comment=Local-first AI collaboration studio
Exec=constellation-desktop
Icon=constellation
Terminal=false
Categories=Development;Utility;
StartupNotify=true
DESKTOP
chmod 0644 "$appdir/app.constellation.studio.desktop"
cp -p "$appdir/app.constellation.studio.desktop" "$appdir/usr/share/applications/"
cat > "$appdir/AppRun" <<'APP_RUN'
#!/bin/sh
set -eu
appdir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)"
exec "$appdir/usr/bin/constellation-desktop" "$@"
APP_RUN
chmod 0755 "$appdir/AppRun"
ln -s constellation.png "$appdir/.DirIcon"

ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$bundle_tool" \
  --runtime-file "$runtime" --no-appstream \
  "$appdir" "$output" 2>&1 | tee "$run/appimage-build.log"
[[ -s "$output" && -x "$output" ]] || exit 3
file "$output" | tee "$run/appimage-file.txt"

# Inspect the embedded SquashFS directly via the AppImage runtime, rootless.
(
  cd "$inspect"
  "$output" --appimage-extract > "$run/appimage-extract.log" 2>&1
)
cmp "$binary" "$inspect/squashfs-root/usr/bin/constellation-desktop"
cmp "$icon" "$inspect/squashfs-root/constellation.png"
cmp "$appdir/AppRun" "$inspect/squashfs-root/AppRun"
test -r "$inspect/squashfs-root/app.constellation.studio.desktop"

sha256sum "$output" | tee "$run/output/SHA256SUMS-appimage.txt"
printf '\nAPPIMAGE_RESULT=PASS\nAPPIMAGE=%s\nAPPIMAGE_SIZE=%s\n' \
  "$output" "$(stat -c %s "$output")"
