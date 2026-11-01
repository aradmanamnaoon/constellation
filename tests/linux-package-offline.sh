#!/usr/bin/env bash
# Ubuntu/WSL Linux packaging fallback when the Linux native Tauri CLI is absent.
# Creates only Linux cache artifacts, leaves the source checkout untouched.
set -Eeuo pipefail
umask 077
export PATH="/home/saba/.cargo/bin:$PATH"

repo="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
cache="${CONSTELLATION_LINUX_CACHE:-/home/saba/.cache}"
[[ "$cache" == /home/saba/.cache ]] || { echo 'Refusing unexpected Linux cache root' >&2; exit 2; }
[[ -d "$repo/apps/desktop/src-tauri" && -d "$repo/apps/desktop/build/_app" ]] || {
  echo 'Frozen Tauri source or prebuilt static frontend absent' >&2; exit 2;
}
[[ "$(uname -m)" == x86_64 ]] || { echo 'Only x86_64 supported' >&2; exit 2; }
for tool in tar cargo dpkg-deb sha256sum file ldd node; do command -v "$tool" >/dev/null; done

mkdir -p "$cache/constellation-linux-packages"
run="$(mktemp -d "$cache/constellation-linux-packages/run.XXXXXXXX")"
snapshot="$run/source"
mkdir -p "$snapshot" "$run/output"
printf '%s\n' "$run" > "$run/RUN_DIR.txt"

# The SHA-bound verification run opts into strict source immutability. Its
# Cargo target is private to that run, preventing cross-agent cache locks.
strict="${CONSTELLATION_LINUX_STRICT_SNAPSHOT:-0}"
[[ "$strict" == 0 || "$strict" == 1 ]] || exit 2
target="${CONSTELLATION_LINUX_TARGET_DIR:-/home/saba/.cache/constellation-linux-desktop}"
[[ "$target" == /home/saba/.cache/constellation-linux-desktop ||
   "$target" == /home/saba/.cache/constellation-linux-desktop-* ]] || {
  echo 'Cargo target must be inside the dedicated Linux cache' >&2; exit 2;
}

sources=(
  apps/desktop/src-tauri/Cargo.toml
  apps/desktop/src-tauri/Cargo.lock
  apps/desktop/src-tauri/tauri.conf.json
  apps/desktop/src-tauri/src/lib.rs
  apps/desktop/src-tauri/src/main.rs
  apps/desktop/build/index.html
  crates/constellation-core/src/lib.rs
  catalog/roles.json
)
for source in "${sources[@]}"; do [[ -f "$repo/$source" ]] || exit 2; done
for source in "${sources[@]}"; do sha256sum "$repo/$source"; done > "$run/source-before.sha256"

if [[ "$strict" == 1 ]]; then
  # Capture every source, asset and lockfile that could contribute to the
  # native or prebuilt frontend. Hash both source snapshots and live inputs.
  included=(apps/desktop/src-tauri apps/desktop/src apps/desktop/build
    apps/desktop/package.json apps/desktop/pnpm-lock.yaml crates catalog/roles.json)
  (
    cd "$repo"
    find "${included[@]}" -type f \
      ! -path '*/target/*' ! -path '*/node_modules/*' \
      ! -path '*/.svelte-kit/*' ! -path '*/dist/*' -print0 |
      sort -z | xargs -0 sha256sum
  ) > "$run/source-full-before.sha256"
fi

# A local snapshot insulates this build from concurrent Windows source edits.
# Never copy Windows targets or node_modules; the frontend has already been built.
tar -C "$repo" --exclude=target --exclude=node_modules --exclude=dist \
  --exclude=.git --exclude=.svelte-kit -cf - \
  apps/desktop/src-tauri apps/desktop/src apps/desktop/package.json \
  apps/desktop/pnpm-lock.yaml apps/desktop/build crates catalog/roles.json |
  tar -C "$snapshot" -xf -
version="$(node -p 'JSON.parse(require("node:fs").readFileSync(process.argv[1],"utf8")).version' \
  "$snapshot/apps/desktop/src-tauri/tauri.conf.json")"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  echo 'Invalid frozen Tauri version: refusing to label Linux packages' >&2; exit 2;
}
for source in "${sources[@]}"; do sha256sum "$repo/$source"; done > "$run/source-after.sha256"
if ! cmp -s "$run/source-before.sha256" "$run/source-after.sha256"; then
  printf 'NOTICE: concurrent edits changed the live checkout during snapshot; build is source-snapshot-specific\n' |
    tee "$run/source-change-warning.txt"
fi
for source in "${sources[@]}"; do
  expected="$(sha256sum "$repo/$source" | cut -d' ' -f1)"
  original="$(sha256sum "$snapshot/$source" | cut -d' ' -f1)"
  if [[ "$expected" != "$original" ]]; then
    printf 'NOTICE: snapshot differs from current checkout: %s\n' "$source" |
      tee -a "$run/source-change-warning.txt"
  fi
done

verify_strict_sources() {
  [[ "$strict" == 1 ]] || return 0
  (
    cd "$repo"
    sha256sum --quiet --check "$run/source-full-before.sha256"
    find "${included[@]}" -type f \
      ! -path '*/target/*' ! -path '*/node_modules/*' \
      ! -path '*/.svelte-kit/*' ! -path '*/dist/*' -print0 |
      sort -z | xargs -0 sha256sum > "$run/source-full-current.sha256"
  )
  cmp "$run/source-full-before.sha256" "$run/source-full-current.sha256" || {
    echo 'SOURCE_DRIFT: live inputs changed; stopping before packaging' >&2; exit 7;
  }
  (cd "$snapshot" && sha256sum --quiet --check "$run/source-full-before.sha256") || {
    echo 'SNAPSHOT_DRIFT: copied sources do not match initial inputs' >&2; exit 7;
  }
  echo SOURCE_SHA_BOUND_PASS
}
verify_strict_sources

export CARGO_TARGET_DIR="$target"
export CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0
export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=8
export CARGO_BUILD_JOBS=2
printf 'Snapshot: %s\nCargo target: %s\n' "$snapshot" "$CARGO_TARGET_DIR"
cargo build --manifest-path "$snapshot/apps/desktop/src-tauri/Cargo.toml" \
  --locked --offline --release --features tauri/custom-protocol 2>&1 | tee "$run/cargo-release-build.log"
verify_strict_sources

binary="$CARGO_TARGET_DIR/release/constellation-desktop"
[[ -f "$binary" && -x "$binary" ]] || { echo 'No linked native Linux executable' >&2; exit 3; }
file "$binary" | tee "$run/linux-binary-file.txt"
ldd "$binary" | tee "$run/linux-binary-ldd.txt"
if grep -q 'not found' "$run/linux-binary-ldd.txt"; then
  echo 'Missing runtime dependency; packaging blocked' >&2; exit 3
fi

pkg="$run/package-root"
mkdir -p "$pkg/DEBIAN" "$pkg/usr/bin" "$pkg/usr/share/applications" \
  "$pkg/usr/share/icons/hicolor/128x128/apps"
chmod 0755 "$pkg" "$pkg/DEBIAN" "$pkg/usr" "$pkg/usr/bin" \
  "$pkg/usr/share" "$pkg/usr/share/applications" "$pkg/usr/share/icons" \
  "$pkg/usr/share/icons/hicolor" "$pkg/usr/share/icons/hicolor/128x128" \
  "$pkg/usr/share/icons/hicolor/128x128/apps"
install -m755 "$binary" "$pkg/usr/bin/constellation-desktop"
install -m644 "$snapshot/apps/desktop/src-tauri/icons/128x128.png" \
  "$pkg/usr/share/icons/hicolor/128x128/apps/constellation.png"
cat > "$pkg/usr/share/applications/app.constellation.studio.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Constellation
Comment=Local-first AI collaboration studio
Exec=/usr/bin/constellation-desktop
Icon=constellation
Terminal=false
Categories=Development;Utility;
StartupNotify=true
DESKTOP
chmod 0644 "$pkg/usr/share/applications/app.constellation.studio.desktop"
installed_kib="$(du -sk "$pkg/usr" | cut -f1)"
cat > "$pkg/DEBIAN/control" <<CONTROL
Package: constellation-studio
Version: ${version}
Section: utils
Priority: optional
Architecture: amd64
Maintainer: Constellation contributors
Installed-Size: ${installed_kib}
Depends: libwebkit2gtk-4.1-0, libgtk-3-0t64, libayatana-appindicator3-1, librsvg2-2
Description: Constellation local-first collaboration desktop
 Unofficial unsigned Linux verification package, built from a frozen
 local source snapshot; no live provider or sync capability is implied.
CONTROL
chmod 0644 "$pkg/DEBIAN/control"
deb="$run/output/constellation-studio_${version}_amd64.deb"
dpkg-deb --root-owner-group --build "$pkg" "$deb"

dpkg-deb --field "$deb" | tee "$run/deb-fields.txt"
dpkg-deb --contents "$deb" | tee "$run/deb-contents.txt"
mkdir -p "$run/extracted"
dpkg-deb --extract "$deb" "$run/extracted"
cmp "$binary" "$run/extracted/usr/bin/constellation-desktop"
cmp "$snapshot/apps/desktop/src-tauri/icons/128x128.png" \
  "$run/extracted/usr/share/icons/hicolor/128x128/apps/constellation.png"
test -f "$run/extracted/usr/share/applications/app.constellation.studio.desktop"
verify_strict_sources
sha256sum "$binary" "$deb" "$run/extracted/usr/bin/constellation-desktop" |
  tee "$run/output/SHA256SUMS.txt"
printf '\nPACKAGE_RESULT=PASS\nPACKAGE_TYPE=manual-dpkg-deb-fallback\nRUN_DIR=%s\nDEB=%s\n' \
  "$run" "$deb"
