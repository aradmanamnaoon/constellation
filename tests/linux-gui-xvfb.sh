#!/usr/bin/env bash
# Exercise an extracted Linux .deb on an isolated Xvfb server. No desktop
# focus, user profile, elevated installation or non-loopback test endpoint.
set -Eeuo pipefail
umask 077

run="${1:-}"
mode="${2:-deb}"
settle_seconds="${3:-3}"
[[ "$settle_seconds" =~ ^([1-9]|1[0-5])$ ]] || {
  echo 'GUI settle seconds must be an integer from 1 to 15' >&2; exit 2;
}
[[ "$run" == /home/saba/.cache/constellation-linux-packages/run.* ]] || {
  echo 'Supply a verified run directory in the dedicated Linux cache' >&2; exit 2;
}
[[ -d "$run" ]] || exit 2
command -v node >/dev/null
version="$(node -p 'JSON.parse(require("node:fs").readFileSync(process.argv[1],"utf8")).version' \
  "$run/source/apps/desktop/src-tauri/tauri.conf.json")"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || exit 2
case "$mode" in
  deb)
    binary="$run/extracted/usr/bin/constellation-desktop"
    private="$run/isolated-gui"
    ;;
  appimage)
    binary="$run/output/Constellation_${version}_x86_64.AppImage"
    private="$run/isolated-gui-appimage"
    ;;
  appimage-fuse)
    binary="$run/output/Constellation_${version}_x86_64.AppImage"
    private="$run/isolated-gui-appimage-fuse"
    ;;
  *) echo 'Mode must be deb, appimage or appimage-fuse' >&2; exit 2 ;;
esac
[[ -f "$binary" && -x "$binary" ]] || { echo 'No verified extracted binary' >&2; exit 2; }
for cmd in xvfb-run xwininfo xwd convert timeout; do command -v "$cmd" >/dev/null; done

mkdir -p "$private/home" "$private/config" "$private/data" "$private/cache" "$private/runtime"
chmod 0700 "$private/runtime"
printf 'Xvfb only; app home=%s\n' "$private/home"

# The normal AppImage path exercises the kernel FUSE runtime; the other
# AppImage mode explicitly uses rootless extraction for environments without
# /dev/fuse. Neither mode changes the host desktop or native package manager.
appimage_env=()
if [[ "$mode" == appimage ]]; then appimage_env=(APPIMAGE_EXTRACT_AND_RUN=1); fi

# Importantly, all runtime discovery is scoped to a disposable HOME. No
# inherited provider keys, proxy settings, browser sessions or display.
env -i \
  "${appimage_env[@]}" \
  PATH=/usr/local/bin:/usr/bin:/bin:/home/saba/.cargo/bin \
  HOME="$private/home" \
  XDG_CONFIG_HOME="$private/config" \
  XDG_DATA_HOME="$private/data" \
  XDG_CACHE_HOME="$private/cache" \
  XDG_RUNTIME_DIR="$private/runtime" \
  TMPDIR="$private/cache" \
  DEBIAN_FRONTEND=noninteractive \
  CONSTELLATION_QA_DIR="$private" \
  CONSTELLATION_QA_BINARY="$binary" \
  CONSTELLATION_QA_SETTLE_SECONDS="$settle_seconds" \
  xvfb-run -a --server-args='-screen 0 1280x800x24 -nolisten tcp' \
  timeout --signal=TERM --kill-after=5s 35s bash -c '
set -Eeuo pipefail
openbox > "$CONSTELLATION_QA_DIR/window-manager.log" 2>&1 &
wm_pid=$!
"$CONSTELLATION_QA_BINARY" > "$CONSTELLATION_QA_DIR/native-console.log" 2>&1 &
app_pid=$!
cleanup() {
  kill "$app_pid" "$wm_pid" 2>/dev/null || true
  wait "$app_pid" "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
observed=0
for attempt in $(seq 1 20); do
  if ! kill -0 "$app_pid" 2>/dev/null; then
    echo "Native process exited before GUI became visible" >&2
    break
  fi
  xwininfo -root -tree > "$CONSTELLATION_QA_DIR/window-tree.txt" 2>&1 || true
  # Match the actual named application window, not the small auxiliary
  # "constellation-desktop" X11 window whose WM_CLASS also contains Constellation.
  window_id="$(awk '\''$2 == "\"Constellation\":" { print $1; exit }'\'' "$CONSTELLATION_QA_DIR/window-tree.txt")"
  if [[ -n "$window_id" ]] &&
     xwininfo -id "$window_id" > "$CONSTELLATION_QA_DIR/window-details.txt" 2>&1; then
    width="$(awk '\''$1 == "Width:" { print $2; exit }'\'' "$CONSTELLATION_QA_DIR/window-details.txt")"
    height="$(awk '\''$1 == "Height:" { print $2; exit }'\'' "$CONSTELLATION_QA_DIR/window-details.txt")"
    if [[ "$width" =~ ^[0-9]+$ && "$height" =~ ^[0-9]+$ ]] &&
       (( width >= 600 && height >= 400 )) &&
       grep -Eq "^[[:space:]]*Map State: IsViewable[[:space:]]*$" "$CONSTELLATION_QA_DIR/window-details.txt"; then
      observed=1
      break
    fi
  fi
  sleep 1
done
if [ "$observed" != 1 ]; then
  echo "GUI_WINDOW_VISIBLE=NO" >&2
  exit 3
fi
sleep "$CONSTELLATION_QA_SETTLE_SECONDS"
if ! kill -0 "$app_pid" 2>/dev/null; then
  echo "Native process exited immediately after window appeared" >&2
  exit 3
fi
xwininfo -root -tree > "$CONSTELLATION_QA_DIR/window-tree.txt"
xwd -root -silent -out "$CONSTELLATION_QA_DIR/root.xwd"
convert "$CONSTELLATION_QA_DIR/root.xwd" "$CONSTELLATION_QA_DIR/screenshot.png"
test -s "$CONSTELLATION_QA_DIR/screenshot.png"
# A mapped window can have an entirely white WebKit viewport while content is
# still loading. Check actual interior pixel diversity as a minimum render
# guard; a three-second FUSE cold-start was observed to fail this condition.
viewport_entropy="$(convert "$CONSTELLATION_QA_DIR/screenshot.png" \
  -crop 950x550+140+150 +repage -format "%[entropy]" info:)"
echo GUI_VIEWPORT_ENTROPY="$viewport_entropy"
if ! awk -v entropy="$viewport_entropy" '\''BEGIN { exit !(entropy > 0.05) }'\''; then
  echo "GUI_WORKSPACE_RENDERED=NO" >&2
  exit 3
fi
sha256sum "$CONSTELLATION_QA_DIR/screenshot.png" > "$CONSTELLATION_QA_DIR/screenshot.sha256"
echo GUI_WINDOW_VISIBLE=YES
echo GUI_WORKSPACE_RENDERED=YES
echo GUI_SCREENSHOT="$CONSTELLATION_QA_DIR/screenshot.png"
'
