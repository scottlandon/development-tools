#!/usr/bin/env bash
# Regenerate the README screenshots in tools/cc-usage/docs/ from the vhs tapes in
# tools/cc-usage/docs/tapes/. Everything is captured from `cc-usage --demo` (synthetic data).
#
# Usage: scripts/screenshots.sh        (from anywhere; it moves to the repo root)
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

if ! command -v vhs >/dev/null 2>&1; then
  echo "error: vhs is not installed (it also needs ttyd and ffmpeg on PATH)." >&2
  echo "  macOS:  brew install vhs      (pulls in ttyd and ffmpeg)" >&2
  echo "  other:  https://github.com/charmbracelet/vhs#installation" >&2
  exit 1
fi

cargo build --release --locked -p cc-usage

# Tapes write throwaway video output here; it lives under target/ (git-ignored) and is removed on exit.
tmp=target/screenshots
mkdir -p "$tmp"
trap 'rm -rf "$tmp"' EXIT

for tape in tools/cc-usage/docs/tapes/*.tape; do
  name=$(basename "$tape" .tape)
  echo "==> $name"
  vhs "$tape" >/dev/null
  png="tools/cc-usage/docs/$name.png"
  if [ ! -s "$png" ]; then
    echo "error: $tape did not produce $png" >&2
    exit 1
  fi
done

echo "Done. Review the images before committing: tools/cc-usage/docs/*.png"
