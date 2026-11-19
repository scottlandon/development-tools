#!/usr/bin/env bash
# Build a tool from this workspace in release mode and install it into a bin directory.
#
#   scripts/install.sh                 # cc-usage -> ~/.local/bin
#   scripts/install.sh cc-usage        # same, named explicitly
#   BINDIR=/usr/local/bin scripts/install.sh
#
# Everything happens on this machine: it builds from the local checkout and copies one file.
set -euo pipefail

tool="${1:-cc-usage}"
bindir="${BINDIR:-$HOME/.local/bin}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

[[ -d "$root/tools/$tool" ]] || { echo "error: no such tool: tools/$tool" >&2; exit 1; }
command -v cargo >/dev/null || { echo "error: cargo not found; install Rust from https://rustup.rs" >&2; exit 1; }

echo "Building $tool (release)..."
cargo build --release --locked -p "$tool" --manifest-path "$root/Cargo.toml"

mkdir -p "$bindir"
install -m 755 "$root/target/release/$tool" "$bindir/$tool"
echo "Installed $bindir/$tool ($("$bindir/$tool" --version))"

case ":$PATH:" in
  *":$bindir:"*) ;;
  *) echo "note: $bindir is not on your PATH. Add this to your shell profile:"
     echo "  export PATH=\"$bindir:\$PATH\"" ;;
esac
