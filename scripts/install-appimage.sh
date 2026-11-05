#!/usr/bin/env bash
# Install a SubForge AppImage for the current user (no root, no package manager):
#   ~/.local/bin/subforge          the application
#   ~/.local/share/applications/   menu entry ("SubForge")
#   ~/.local/share/icons/hicolor/  icons, so it is not a blank square
#
# Usage:
#   bash install-appimage.sh subforge_0.3.0_amd64.AppImage
#   bash install-appimage.sh                 # picks the newest subforge*.AppImage
#                                            # in the current directory or ~/Downloads
#   bash install-appimage.sh --uninstall
#
# Run it again with a newer AppImage to update: SubForge itself does not auto-update.
set -euo pipefail

app_name=subforge
bin_dir=${XDG_BIN_HOME:-$HOME/.local/bin}
data_dir=${XDG_DATA_HOME:-$HOME/.local/share}
desktop_dir=$data_dir/applications
installed_bin=$bin_dir/subforge

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() { sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; }

refresh_caches() {
  command -v update-desktop-database >/dev/null 2>&1 &&
    update-desktop-database "$desktop_dir" 2>/dev/null || true
  command -v gtk-update-icon-cache >/dev/null 2>&1 &&
    gtk-update-icon-cache -q -t "$data_dir/icons/hicolor" 2>/dev/null || true
}

uninstall() {
  rm -f "$installed_bin" "$desktop_dir/subforge.desktop"
  find "$data_dir/icons/hicolor" -name 'subforge.png' -delete 2>/dev/null || true
  refresh_caches
  echo "Removed SubForge from $bin_dir and the application menu."
  echo "Settings stay in ${XDG_CONFIG_HOME:-$HOME/.config}/subforge."
}

case ${1:-} in
  -h | --help | help) usage; exit 0 ;;
  -u | --uninstall | uninstall) uninstall; exit 0 ;;
esac

if [ -n "${1:-}" ]; then
  appimage=$1
else
  found=$(ls -1t ./subforge*.AppImage "$HOME/Downloads"/subforge*.AppImage 2>/dev/null | head -n1 || true)
  [ -n "$found" ] || die "no subforge*.AppImage given or found in the current directory or ~/Downloads"
  appimage=$found
fi

[ -f "$appimage" ] || die "$appimage: no such file"
appimage=$(readlink -f "$appimage")

chmod +x "$appimage" 2>/dev/null ||
  die "$appimage: cannot be made executable (read-only location?) — copy it to your home directory first"

# The AppImage runtime needs FUSE 2 to mount itself. `--appimage-extract` works
# without it, and is all this script needs, but a first launch will fail without
# libfuse.so.2 on distributions that no longer install it by default.
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

icon_src=$work/icons
if ( cd "$work" && "$appimage" --appimage-extract 'usr/share/icons/hicolor/*' >/dev/null 2>&1 ); then
  icon_src=$work/squashfs-root/usr/share/icons/hicolor
fi

mkdir -p "$bin_dir" "$desktop_dir"

# Install the AppImage itself. Kill any running copy first: replacing the file
# under a running process leaves a stale mapping.
pkill -x subforge >/dev/null 2>&1 || true
install -m 755 "$appimage" "$installed_bin"

if [ -d "$icon_src" ]; then
  while IFS= read -r icon; do
    size=$(basename "$(dirname "$(dirname "$icon")")")
    install -Dm644 "$icon" "$data_dir/icons/hicolor/$size/apps/subforge.png"
  done < <(find "$icon_src" -path '*/apps/subforge.png' -type f)
fi

# scripts/install-desktop-launcher.sh (source checkouts) writes the same file name;
# one entry is what we want, so say plainly that it is being replaced.
if [ -f "$desktop_dir/subforge.desktop" ] && grep -q 'launch-subforge.sh' "$desktop_dir/subforge.desktop"; then
  echo "note: replacing the development launcher entry installed by scripts/install-desktop-launcher.sh"
fi

cat >"$desktop_dir/subforge.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=SubForge
GenericName=Subtitle and audio toolkit
Comment=Extract, convert, clean, sync, transcribe and translate subtitles
Exec=$installed_bin
Icon=subforge
Terminal=false
Categories=AudioVideo;
Keywords=subtitle;srt;ass;mkv;ffmpeg;transcribe;translate;
StartupWMClass=subforge
EOF
chmod 644 "$desktop_dir/subforge.desktop"

refresh_caches

if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$desktop_dir/subforge.desktop" || true
fi

echo "Installed: $installed_bin"
echo "Menu entry: $desktop_dir/subforge.desktop"
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "note: $bin_dir is not in PATH, so run it from the application menu or as $installed_bin" ;;
esac
if [ ! -e /usr/lib/libfuse.so.2 ] && [ ! -e /usr/lib64/libfuse.so.2 ] &&
  ! ldconfig -p 2>/dev/null | grep -q 'libfuse\.so\.2'; then
  echo "note: FUSE 2 is missing, so the AppImage cannot mount itself."
  echo "      Arch: sudo pacman -S fuse2   Debian/Ubuntu: sudo apt install libfuse2"
  echo "      or launch it once with: $installed_bin --appimage-extract-and-run"
fi
