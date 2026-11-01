#!/usr/bin/env bash
# Reproduce the source, crate, and native Rust gates locally in Ubuntu WSL.
# No sudo, network fetching, cache deletion, Git mutation, signing, or release.
set -Eeuo pipefail
# Non-login WSL shells do not necessarily source ~/.profile, which normally
# adds rustup's Cargo binaries to PATH.
export PATH="$HOME/.cargo/bin:$PATH"

usage() {
  echo 'Usage: bash scripts/qa/run-ubuntu-wsl-ci.sh [preflight|crates|desktop]'
  echo '  preflight  Read-only source, version, and disk checks'
  echo '  crates     Node contracts plus all 14 standalone Rust crates (default)'
  echo '  desktop    Rust desktop format, strict Clippy, lib tests, and dev build'
  echo 'For desktop, set CONSTELLATION_WSL_DESKTOP_TARGET_DIR to an existing Linux build cache.'
}

mode="${1:-crates}"
case "$mode" in
  preflight|crates|desktop) ;;
  -h|--help) usage; exit 0 ;;
  *) usage >&2; exit 64 ;;
esac
if [[ "$(uname -s)" != Linux ]]; then
  echo 'This runner requires Linux/Ubuntu; it does not certify macOS or Windows.' >&2
  exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
cd "$root"
for program in git cargo node df timeout; do
  command -v "$program" >/dev/null || { echo "Missing required tool: $program" >&2; exit 2; }
done

# The WSL ext4 filesystem reports virtual capacity; its growing VHDX is
# physically backed by Windows. Check host drives before EACH heavy stage.
disk_guard() {
  if [[ -e /proc/sys/fs/binfmt_misc/WSLInterop ]]; then
    local mount free
    for mount in /mnt/c /mnt/d; do
      [[ -d "$mount" ]] || continue
      free="$(df -PB1 "$mount" | awk 'NR == 2 {print $4}')"
      [[ "$free" =~ ^[0-9]+$ ]] || { echo "Cannot inspect $mount disk" >&2; exit 2; }
      if [[ "$mount" == /mnt/c && "$free" -lt 3221225472 ]] ||
         [[ "$mount" == /mnt/d && "$free" -lt 83886080 ]]; then
        echo "Low physical Windows space on $mount: $free bytes. Refusing compilation." >&2
        exit 2
      fi
    done
  fi
}

disk_guard
echo "Source: $(git rev-parse HEAD)"
echo "Rust: $(rustc --version)"
echo "Node: $(node --version)"
node .github/workflows/source-gate.mjs
# The checkout is shared with Windows Git, which writes CRLF for files without
# explicit LF attributes. Avoid treating its ordinary CRLF checkout as a huge
# synthetic diff under Ubuntu's default core.autocrlf=false. This affects only
# this read-only invocation, not the user's repository or global Git settings.
git -c core.autocrlf=true diff --check
if [[ "$mode" == preflight ]]; then
  echo WSL_PREFLIGHT_PASS
  exit 0
fi

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/constellation-linux-target}"

require_linux_cache() {
  local target
  target="$(realpath -m "$CARGO_TARGET_DIR")"
  case "$target" in
    /mnt/c/*|/mnt/d/*) echo 'CARGO_TARGET_DIR must use Linux ext4, not a Windows-mounted drive.' >&2; exit 2 ;;
  esac
}

if [[ "$mode" == crates ]]; then
  require_linux_cache
  node --test tests/*.test.mjs
  for manifest in crates/*/Cargo.toml; do
    disk_guard
    echo "Checking $manifest"
    cargo fmt --manifest-path "$manifest" --all --check
    if [[ "$manifest" == crates/constellation-project-backup/Cargo.toml ]]; then
      timeout 360s cargo test --manifest-path "$manifest" --lib --locked --offline
      timeout 360s cargo test --manifest-path "$manifest" --test project_archive --locked --offline
    else
      timeout 360s cargo test --manifest-path "$manifest" --all-targets --locked --offline
    fi
    timeout 360s cargo clippy --manifest-path "$manifest" --all-targets --locked --offline -- -D warnings
  done
  echo WSL_CRATE_GATES_PASS
  exit 0
fi

# Do not unknowingly duplicate many gigabytes of cached Tauri dependencies.
if [[ -z "${CONSTELLATION_WSL_DESKTOP_TARGET_DIR:-}" ||
      ! -d "$CONSTELLATION_WSL_DESKTOP_TARGET_DIR" ]]; then
  echo 'Set CONSTELLATION_WSL_DESKTOP_TARGET_DIR to an existing Linux desktop Cargo cache.' >&2
  exit 2
fi
export CARGO_TARGET_DIR="$CONSTELLATION_WSL_DESKTOP_TARGET_DIR"
require_linux_cache
disk_guard
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --all --check
timeout 360s cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked --offline -- -D warnings
disk_guard
timeout 360s cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib --locked --offline
disk_guard
timeout 360s cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --offline
echo WSL_DESKTOP_RUST_GATES_PASS
echo 'This is NOT installed GUI acceptance, Android certification, or a signed production package.'
