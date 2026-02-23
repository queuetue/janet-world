#!/usr/bin/env bash
# build.sh — compile janet-world-wasm and optionally copy output into a web project.
#
# Usage:
#   ./build.sh [--release] [--web-project PATH] [--target TARGET]
#
# Environment:
#   WEB_PROJECT     Path to your web project root (default: none)
#   WASM_TARGET     wasm-pack target: web | bundler | nodejs | no-modules
#                   (default: web — ES module with top-level await)
#
# After running, import from:
#   pkg/janet_world_wasm.js        (JS bindings)
#   pkg/janet_world_wasm_bg.wasm   (compiled module)
#
# Prerequisites:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-pack

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

PROFILE="dev"
WASM_PACK_FLAGS=()
WEB_PROJECT="${WEB_PROJECT:-}"
WASM_TARGET="${WASM_TARGET:-web}"

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      PROFILE="release"
      WASM_PACK_FLAGS+=(--release)
      shift ;;
    --web-project)
      WEB_PROJECT="$2"
      shift 2 ;;
    --target)
      WASM_TARGET="$2"
      shift 2 ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1 ;;
  esac
done

if [[ "$PROFILE" == "dev" ]]; then
  WASM_PACK_FLAGS+=(--dev)
fi

# ---------------------------------------------------------------------------
# Check prerequisites
# ---------------------------------------------------------------------------
if ! command -v wasm-pack >/dev/null 2>&1; then
  echo "Error: wasm-pack not found."
  echo "Install: cargo install wasm-pack"
  exit 1
fi

if ! rustup target list --installed | grep -q wasm32-unknown-unknown; then
  echo "Adding wasm32-unknown-unknown target..."
  rustup target add wasm32-unknown-unknown
fi

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
echo "==> Building janet-world-wasm ($PROFILE, target=$WASM_TARGET)..."
(
  cd "$SCRIPT_DIR"
  wasm-pack build --target "$WASM_TARGET" "${WASM_PACK_FLAGS[@]}"
)

echo "==> Output written to $SCRIPT_DIR/pkg/"

# ---------------------------------------------------------------------------
# Sync into web project (optional)
# ---------------------------------------------------------------------------
if [[ -n "$WEB_PROJECT" ]]; then
  DEST="$WEB_PROJECT/src/janet_world_wasm"
  echo "==> Installing into $DEST ..."
  mkdir -p "$DEST"
  # Copy the JS + WASM artefacts; skip the package.json (version pinned elsewhere)
  cp "$SCRIPT_DIR/pkg/janet_world_wasm.js"       "$DEST/"
  cp "$SCRIPT_DIR/pkg/janet_world_wasm_bg.wasm"  "$DEST/"
  cp "$SCRIPT_DIR/pkg/janet_world_wasm.d.ts"     "$DEST/" 2>/dev/null || true
  echo "==> Done.  Import from: $DEST/janet_world_wasm.js"
else
  echo ""
  echo "Tip: pass --web-project /path/to/project to auto-install the output."
fi

# ---------------------------------------------------------------------------
# NATS prerequisite reminder
# ---------------------------------------------------------------------------
echo ""
echo "Remember: start NATS with WebSocket support before connecting:"
echo "  nats-server --websocket --wsport 9222"
