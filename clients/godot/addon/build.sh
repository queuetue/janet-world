#!/usr/bin/env bash
# build.sh — compile the Janet World GDExtension and install it into a Godot project.
#
# Usage:
#   ./build.sh [--release] [--godot-project PATH]
#
# Environment:
#   GODOT_PROJECT   Path to the root of your Godot 4 project (default: ../../../demo)
#
# The script will:
#   1. Build the Rust crate (debug or release)
#   2. Copy the compiled .so / .dylib / .dll into the addon/bin/ directory
#   3. If GODOT_PROJECT is set, sync the entire addon/ tree into that project

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$SCRIPT_DIR/.."          # clients/godot/
ADDON_DIR="$SCRIPT_DIR"             # clients/godot/addon/
BIN_DIR="$ADDON_DIR/bin"

PROFILE="debug"
CARGO_FLAGS=()
GODOT_PROJECT="${GODOT_PROJECT:-}"

# --------------------------------------------------------------------------- #
#  Argument parsing
# --------------------------------------------------------------------------- #
while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      PROFILE="release"
      CARGO_FLAGS+=(--release)
      shift ;;
    --godot-project)
      GODOT_PROJECT="$2"
      shift 2 ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1 ;;
  esac
done

# --------------------------------------------------------------------------- #
#  Build
# --------------------------------------------------------------------------- #
echo "==> Building janet-godot-world ($PROFILE)..."
(cd "$CRATE_DIR" && cargo build "${CARGO_FLAGS[@]}")

TARGET_DIR="$CRATE_DIR/target/$PROFILE"
mkdir -p "$BIN_DIR"

# Detect platform and copy the right artifact
case "$(uname -s)" in
  Linux*)
    SRC="$TARGET_DIR/libjanet_godot_world.so"
    DST="$BIN_DIR/linux.debug.x86_64.so"
    if [[ "$PROFILE" == "release" ]]; then DST="$BIN_DIR/linux.release.x86_64.so"; fi
    cp -v "$SRC" "$DST" ;;
  Darwin*)
    SRC="$TARGET_DIR/libjanet_godot_world.dylib"
    DST="$BIN_DIR/macos.debug.dylib"
    if [[ "$PROFILE" == "release" ]]; then DST="$BIN_DIR/macos.release.dylib"; fi
    cp -v "$SRC" "$DST" ;;
  MINGW*|CYGWIN*|MSYS*)
    SRC="$TARGET_DIR/janet_godot_world.dll"
    DST="$BIN_DIR/windows.debug.x86_64.dll"
    if [[ "$PROFILE" == "release" ]]; then DST="$BIN_DIR/windows.release.x86_64.dll"; fi
    cp -v "$SRC" "$DST" ;;
  *)
    echo "Unsupported platform: $(uname -s)" >&2
    exit 1 ;;
esac

echo "==> Library installed at $DST"

# --------------------------------------------------------------------------- #
#  Sync addon into Godot project (optional)
# --------------------------------------------------------------------------- #
if [[ -n "$GODOT_PROJECT" ]]; then
  DEST="$GODOT_PROJECT/addons/janet_world"
  echo "==> Syncing addon to $DEST ..."
  mkdir -p "$DEST"
  rsync -av --delete "$ADDON_DIR/" "$DEST/"
  echo "==> Done. Open Godot → Project Settings → Plugins and enable 'Janet World'."
else
  echo ""
  echo "Tip: pass --godot-project /path/to/godot-project to auto-install into addons/janet_world/"
fi
