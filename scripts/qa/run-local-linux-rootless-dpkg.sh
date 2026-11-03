#!/usr/bin/env bash
# Rootless, isolated Debian dpkg package-install verification. Does not touch
# the host's /var/lib/dpkg, system package database, /usr, or user application data.
set -Eeuo pipefail
umask 077

deb="${1:-/mnt/d/coding/codex/constellation-ux-rebuild/target-linux/release/bundle/deb/Constellation_0.1.2_amd64.deb}"
[[ -f "$deb" ]] || { echo "Missing requested .deb" >&2; exit 2; }
[[ "$(dpkg-deb --field "$deb" Package)" == 'constellation' ]] || exit 2
[[ "$(dpkg-deb --field "$deb" Architecture)" == 'amd64' ]] || exit 2

# Maintainer hooks would run during unpack; refuse any archive containing them.
control="$(dpkg-deb --ctrl-tarfile "$deb" | tar -tf -)"
while IFS= read -r item; do
  case "$item" in
    ./|control|./control|md5sums|./md5sums|conffiles|./conffiles) ;;
    *) echo "Unexpected Debian control file: $item" >&2; exit 2 ;;
  esac
done <<< "$control"

root="$(mktemp -d /home/saba/.cache/constellation-linux-packages/qa-rootless-install.XXXXXXXX)"
[[ "$root" == /home/saba/.cache/constellation-linux-packages/qa-rootless-install.* ]] || exit 2
mkdir -p "$root/var/lib/dpkg/updates" "$root/var/lib/dpkg/info"
: > "$root/var/lib/dpkg/status"

dpkg --root="$root" --force-not-root --log="$root/dpkg-install.log" --unpack "$deb" \
  | tee "$root/dpkg-unpack.log"
status="$(dpkg --root="$root" --status constellation)"
grep -q 'Status: install ok unpacked' <<< "$status"

installed="$root/usr/bin/constellation-desktop"
[[ -s "$installed" && -x "$installed" ]] || { echo 'Expected executable absent' >&2; exit 3; }
tmp="$(mktemp -d /home/saba/.cache/constellation-linux-packages/qa-raw-extract.XXXXXXXX)"
dpkg-deb --extract "$deb" "$tmp"
cmp "$installed" "$tmp/usr/bin/constellation-desktop"
test -r "$root/usr/share/applications/Constellation.desktop"

printf 'ROOTLESS_DPKG_UNPACK_PASS\nPACKAGE_SHA256=%s\nINSTALLED_BINARY_SHA256=%s\nROOT=%s\n' \
  "$(sha256sum "$deb" | cut -d' ' -f1)" \
  "$(sha256sum "$installed" | cut -d' ' -f1)" "$root"
