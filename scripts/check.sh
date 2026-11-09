#!/usr/bin/env bash
# Glowtune: full check (format, lint, test, frontend type-check + build).
# Works in Git Bash on Windows and natively on Linux. CI-friendly: exits non-zero on failure.
#
#   ./scripts/check.sh          # everything
#   ./scripts/check.sh rust     # Rust only
#   ./scripts/check.sh ui       # frontend only
set -euo pipefail
cd "$(dirname "$0")/.."

what="${1:-all}"
step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }

if [[ "$what" == "all" || "$what" == "rust" ]]; then
  step "cargo fmt --check"
  cargo fmt --all --check
  step "cargo clippy (deny warnings)"
  cargo clippy --workspace --all-targets -- -D warnings
  step "cargo test"
  cargo test --workspace
fi

if [[ "$what" == "all" || "$what" == "ui" ]]; then
  step "pnpm install (frozen lockfile)"
  pnpm -C app install --frozen-lockfile
  step "svelte-check"
  pnpm -C app check
  step "music metadata extractor"
  node --test app/src-tauri/src/music_snapshot.test.cjs
  step "vite build"
  pnpm -C app build
fi

step "all checks passed ✔"
