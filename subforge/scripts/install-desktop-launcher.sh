#!/usr/bin/env bash
#
# Install a .desktop launcher for SubForge into the user's home directory.
# Run once; idempotent (safe to re-run).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

ICON_SOURCE="${PROJECT_ROOT}/src-tauri/icons/icon.png"
ICON_NAME="com.updullah.subforge.png"
ICON_SIZE="512x512"
ICON_DEST_DIR="${HOME}/.local/share/icons/hicolor/${ICON_SIZE}/apps"
ICON_DEST="${ICON_DEST_DIR}/${ICON_NAME}"

DESKTOP_DIR="${HOME}/.local/share/applications"
DESKTOP_FILE="${DESKTOP_DIR}/subforge.desktop"
LAUNCHER_SCRIPT="${PROJECT_ROOT}/scripts/launch-subforge.sh"

# ── Validate prerequisites ──────────────────────────────────────────────────
if [[ ! -f "$ICON_SOURCE" ]]; then
    echo "Error: Icon not found at ${ICON_SOURCE}" >&2
    exit 1
fi

if [[ ! -f "$LAUNCHER_SCRIPT" ]]; then
    echo "Error: Launcher script not found at ${LAUNCHER_SCRIPT}" >&2
    exit 1
fi

# Make launcher executable
chmod +x "$LAUNCHER_SCRIPT"

# ── Install icon ────────────────────────────────────────────────────────────
mkdir -p "$ICON_DEST_DIR"
cp -f "$ICON_SOURCE" "$ICON_DEST"
echo "✓ Installed icon → ${ICON_DEST}"

# ── Generate .desktop file ──────────────────────────────────────────────────
mkdir -p "$DESKTOP_DIR"

cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Version=1.0
Name=SubForge
Comment=Subtitle & audio toolkit
Exec=${LAUNCHER_SCRIPT}
Icon=${ICON_NAME%.png}
Type=Application
Categories=AudioVideo;Utility;
Terminal=false
StartupNotify=true
EOF

echo "✓ Installed desktop entry → ${DESKTOP_FILE}"

# ── Update desktop database ─────────────────────────────────────────────────
if command -v update-desktop-database &>/dev/null; then
    update-desktop-database "$DESKTOP_DIR"
    echo "✓ Updated desktop database"
else
    echo "⚠ update-desktop-database not found; you may need to log out and back in for the icon to appear."
fi

# ── Update icon cache ───────────────────────────────────────────────────────
if command -v gtk-update-icon-cache &>/dev/null; then
    gtk-update-icon-cache -f -t "${HOME}/.local/share/icons/hicolor" 2>/dev/null || true
    echo "✓ Updated icon cache"
fi

echo ""
echo "SubForge launcher installed successfully!"
echo ""
echo "  • App menu entry: SubForge"
echo "  • Desktop file:   ${DESKTOP_FILE}"
echo "  • Launch script:  ${LAUNCHER_SCRIPT}"
echo ""
echo "You can also run the script directly:"
echo "  ${LAUNCHER_SCRIPT}           # Tauri desktop mode"
echo "  ${LAUNCHER_SCRIPT} --server  # Axum server mode"
