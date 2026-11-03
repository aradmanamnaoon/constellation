#!/usr/bin/env bash
# Build the current Windows-worktree sources in a real WSL2 Ubuntu userspace.
# Do not reuse the Windows node_modules or Rust target artifacts.
set -Eeuo pipefail
umask 077
source_root=/mnt/d/coding/codex/constellation-ux-rebuild
linux_root=/home/saba/constellation-ux-linux
[[ -f "$source_root/apps/desktop/package.json" ]] || exit 2
[[ -d "$linux_root/node_modules" || -d "$linux_root/apps/desktop/node_modules" ]] || {
  echo 'Run the initial Linux dependency installation before this rebuild.' >&2
  exit 2
}
cd "$source_root"
# Exclude volatile generated frontend outputs too. A concurrent Windows Vite
# build may rewrite them during this source copy, causing tar to fail with
# "file changed as we read it" even though tracked sources are unchanged.
tar --exclude=.git --exclude=node_modules --exclude=target --exclude=target-linux --exclude=target-android \
    --exclude='./apps/desktop/.svelte-kit' --exclude='./apps/desktop/build' \
    --exclude='./apps/desktop/dist' --exclude='./apps/desktop/src-tauri/target' \
    -cf - . | tar -xf - -C "$linux_root"
cd "$linux_root"
export PATH="/home/saba/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$source_root/target-linux"
export CARGO_INCREMENTAL=0
pnpm --dir apps/desktop check
pnpm --dir apps/desktop tauri build --bundles "${1:-deb,appimage}"
