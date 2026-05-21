#!/usr/bin/env bash
#
# SubForge launcher — auto-detects release binary, dev mode, or server mode.
# Usage: ./launch-subforge.sh [--server]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

SERVER_MODE=false
for arg in "$@"; do
    if [[ "$arg" == "--server" ]]; then
        SERVER_MODE=true
    fi
done

# ── Helper: show error dialog if available ──────────────────────────────────
show_error() {
    local msg="$1"
    echo "ERROR: $msg" >&2
    if command -v zenity &>/dev/null; then
        zenity --error --title="SubForge" --text="$msg" 2>/dev/null || true
    elif command -v notify-send &>/dev/null; then
        notify-send --app-name="SubForge" --urgency=critical "SubForge" "$msg" || true
    fi
}

# ── Server mode ─────────────────────────────────────────────────────────────
if [[ "$SERVER_MODE" == true ]]; then
    if ! command -v cargo &>/dev/null; then
        show_error "Rust/Cargo not found. Cannot start SubForge server."
        exit 1
    fi
    cd "$PROJECT_ROOT/server"
    exec cargo run --release
fi

# ── Desktop mode: try release binary first ──────────────────────────────────
RELEASE_DIR="${PROJECT_ROOT}/src-tauri/target/release"
APP_IMAGE_DIR="${RELEASE_DIR}/bundle/appimage"

# 1. AppImage bundle
if [[ -d "$APP_IMAGE_DIR" ]]; then
    APPIMAGE=$(find "$APP_IMAGE_DIR" -maxdepth 1 -name "*.AppImage" -type f -print -quit 2>/dev/null || true)
    if [[ -n "${APPIMAGE:-}" && -f "$APPIMAGE" ]]; then
        exec "$APPIMAGE"
    fi
fi

# 2. Direct binary
BINARY="${RELEASE_DIR}/subforge"
if [[ -f "$BINARY" && -x "$BINARY" ]]; then
    exec "$BINARY"
fi

# ── Fall back to dev mode ───────────────────────────────────────────────────
if ! command -v npm &>/dev/null; then
    show_error "No release binary found and npm is not available.\n\nPlease build the app first:\n  cd ${PROJECT_ROOT}\n  npm run tauri build"
    exit 1
fi

if ! command -v cargo &>/dev/null; then
    show_error "No release binary found and Rust/Cargo is not available.\n\nPlease install Rust: https://rustup.rs/"
    exit 1
fi

# Check for Wayland fix
if [[ "${XDG_SESSION_TYPE:-}" == "wayland" ]]; then
    export WEBKIT_DISABLE_DMABUF_RENDERER=1
    export WEBKIT_DISABLE_COMPOSITING_MODE=1
fi

# Check if dev port is already occupied (prevents cryptic Vite crash)
DEV_PORT=1422
if command -v ss &>/dev/null; then
    OCCUPIED=$(ss -tlnp 2>/dev/null | grep -E "\[?::\]?:${DEV_PORT} " | head -1 || true)
    if [[ -n "${OCCUPIED:-}" ]]; then
        # Try to extract the process name
        PROC=$(echo "$OCCUPIED" | sed -n 's/.*users:(("\([^"]*\)".*/\1/p')
        if [[ -z "${PROC:-}" ]]; then
            PROC="unknown process"
        fi
        show_error "Cannot start SubForge in dev mode.\n\nPort ${DEV_PORT} is already in use by: ${PROC}\n\nPlease close that program first, or build a release binary:\n  cd ${PROJECT_ROOT}\n  npm run tauri build"
        exit 1
    fi
fi

cd "$PROJECT_ROOT"
exec npm run tauri:dev
