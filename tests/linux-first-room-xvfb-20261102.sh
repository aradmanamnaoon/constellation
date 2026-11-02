#!/usr/bin/env bash
# Disposable Linux-only Xvfb smoke for the first-room onboarding dialog.
# Never uses the physical display, keyboard, pointer, account, or package manager.
set -Eeuo pipefail
umask 077

run="${1:-}"
expected_sha="${2:-}"
mode="${3:-inspect}"
suffix="${4:-}"
[[ "$run" == /home/saba/.cache/constellation-linux-packages/run.* ]] || exit 2
[[ "$mode" == inspect || "$mode" == create ]] || exit 2
[[ -z "$suffix" || "$suffix" =~ ^[a-z0-9-]{1,24}$ ]] || exit 2
[[ "$expected_sha" =~ ^[a-fA-F0-9]{64}$ ]] || exit 2
binary="$run/extracted/usr/bin/constellation-desktop"
[[ -x "$binary" ]] || exit 2
[[ "$(sha256sum "$binary" | cut -d' ' -f1)" == "${expected_sha,,}" ]] || {
  echo 'Executable SHA-256 mismatch; refusing native launch' >&2; exit 2;
}
for command in xvfb-run openbox xdotool xwininfo xwd convert timeout python3; do
  command -v "$command" >/dev/null || exit 2
done
private="$run/isolated-first-room-$mode${suffix:+-$suffix}"
[[ ! -e "$private" ]] || { echo 'Private QA destination already exists' >&2; exit 2; }
mkdir -p "$private"/{home,config,data,cache,runtime}
chmod 0700 "$private/runtime"

env -i PATH=/usr/local/bin:/usr/bin:/bin \
  HOME="$private/home" XDG_CONFIG_HOME="$private/config" \
  XDG_DATA_HOME="$private/data" XDG_CACHE_HOME="$private/cache" \
  XDG_RUNTIME_DIR="$private/runtime" TMPDIR="$private/cache" \
  QA_MODE="$mode" QA_PRIVATE="$private" QA_BINARY="$binary" \
  xvfb-run -a --server-args='-screen 0 1280x800x24 -nolisten tcp' \
  timeout --signal=TERM --kill-after=5s 45s bash -c '
set -Eeuo pipefail
openbox > "$QA_PRIVATE/window-manager.log" 2>&1 &
wm_pid=$!
"$QA_BINARY" > "$QA_PRIVATE/native-console.log" 2>&1 &
app_pid=$!
cleanup() {
  kill "$app_pid" "$wm_pid" 2>/dev/null || true
  wait "$app_pid" "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
window=""
for attempt in $(seq 1 24); do
  if ! kill -0 "$app_pid" 2>/dev/null; then echo APP_EXITED_EARLY >&2; exit 3; fi
  window="$(xdotool search --onlyvisible --name "^Constellation$" 2>/dev/null | head -n 1 || true)"
  if [[ "$window" =~ ^[0-9]+$ ]] && xwininfo -id "$window" > "$QA_PRIVATE/window-info.txt" 2>&1; then
    break
  fi
  sleep 0.5
done
[[ "$window" =~ ^[0-9]+$ ]] || { echo APP_WINDOW_ABSENT >&2; exit 3; }
width="$(awk '\''$1 == "Width:" { print $2; exit }'\'' "$QA_PRIVATE/window-info.txt")"
height="$(awk '\''$1 == "Height:" { print $2; exit }'\'' "$QA_PRIVATE/window-info.txt")"
[[ "$width" =~ ^[0-9]+$ && "$height" =~ ^[0-9]+$ ]] || exit 3
(( width >= 1000 && height >= 650 )) || { echo UNEXPECTED_WINDOW_SIZE >&2; exit 3; }
sleep 2
# This pointer exists only in a separate Xvfb virtual display, not the host.
# Coordinate is grounded in the visually inspected 1280x800 first-run layout.
xdotool mousemove 730 640 click 1
sleep 1
xwd -root -silent -out "$QA_PRIVATE/dialog.xwd"
convert "$QA_PRIVATE/dialog.xwd" "$QA_PRIVATE/dialog.png"
echo QA_DIALOG_SCREENSHOT="$QA_PRIVATE/dialog.png"
if [[ "$QA_MODE" == create ]]; then
  # These coordinates were verified from the preceding private Xvfb dialog
  # screenshot, not guessed from a physical desktop or a CSS-only assertion.
  xdotool mousemove 595 473 click 1
  xdotool type --clearmodifiers --delay 10 "Constellation QA room"
  xdotool mousemove 635 538 click 1
  sleep 2
  xwd -root -silent -out "$QA_PRIVATE/after.xwd"
  convert "$QA_PRIVATE/after.xwd" "$QA_PRIVATE/after.png"
  echo QA_AFTER_SCREENSHOT="$QA_PRIVATE/after.png"
fi
if ! kill -0 "$app_pid" 2>/dev/null; then echo APP_PROCESS_EXITED >&2; exit 3; fi
echo NATIVE_PRIVATE_XVFB=PASS
'
echo QA_PRIVATE_DIR="$private"
if [[ "$mode" == create ]]; then
  python3 - "$private" <<'PY'
import json
from pathlib import Path
import sqlite3
import sys

private = Path(sys.argv[1])
databases = list(private.rglob('constellation.sqlite3'))
if len(databases) != 1:
    raise SystemExit(f'Expected exactly one private native SQLite project, got {len(databases)}')
with sqlite3.connect(databases[0]) as connection:
    rows = connection.execute('SELECT data FROM rooms').fetchall()
if len(rows) != 1:
    raise SystemExit(f'Expected exactly one saved room, got {len(rows)}')
room = json.loads(rows[0][0])
if room.get('title') != 'Constellation QA room':
    raise SystemExit(f'Room identity mismatch, keys={list(room)}')
print('NATIVE_SQLITE_FIRST_ROOM=PASS')
print('NATIVE_ROOM_COUNT=1')
PY
fi
