#!/usr/bin/env bash
# Rootless Debian unpack in a disposable dpkg database and filesystem root.
# Does not change host /usr, /etc, dpkg database or APT installation.
set -Eeuo pipefail
umask 077
run="${1:-}"
[[ "$run" == /home/saba/.cache/constellation-linux-packages/run.* ]] || {
  echo 'Unexpected cache run path' >&2; exit 2;
}
deb="$run/output/constellation-studio_0.1.0_amd64.deb"
[[ -f "$deb" && -d "$run/extracted" ]] || exit 2
root="$run/rootless-dpkg-root"
[[ ! -e "$root" ]] || { echo 'Rootless package test already exists; refusing overwrite' >&2; exit 2; }

# No maintainer scripts or host hooks are authorized in this exercise.
entries="$(dpkg-deb --ctrl-tarfile "$deb" | tar -tf -)"
[[ "$entries" == $'./\n./control' || "$entries" == './control' ]] || {
  echo 'Unexpected package control hook: refusing to unpack' >&2; exit 2;
}
mkdir -p "$root/var/lib/dpkg/updates" "$root/var/lib/dpkg/info"
: > "$root/var/lib/dpkg/status"

# dpkg --root changes both installation root and administrative database.
# --force-not-root only allows this otherwise isolated user-owned tree.
dpkg --root="$root" --force-not-root --log="$run/rootless-dpkg.log" --unpack "$deb" |
  tee "$run/rootless-dpkg-unpack.log"
dpkg --root="$root" --status constellation-studio |
  tee "$run/rootless-dpkg-status.txt"
grep -q 'Status: install ok unpacked' "$run/rootless-dpkg-status.txt"
cmp "$run/extracted/usr/bin/constellation-desktop" "$root/usr/bin/constellation-desktop"
test -r "$root/usr/share/applications/app.constellation.studio.desktop"
printf 'ROOTLESS_DPKG_UNPACK=PASS\nROOT=%s\n' "$root"
