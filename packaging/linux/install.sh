#!/usr/bin/env bash
# Installs Drift for the current user: binary in ~/.local/bin, desktop entry and icon in
# ~/.local/share. No root needed, and the built in updater can replace the binary later.
#
# Usage: install.sh [BINARY]     (default: ./drift in a release tarball, target/release/drift in the repo)
#        install.sh --uninstall
# Works from the repo (packaging/linux/install.sh) and from an unpacked release tarball.
# Set PREFIX to install somewhere other than ~/.local.
set -euo pipefail

prefix="${PREFIX:-$HOME/.local}"
# the desktop file and icon sit next to this script in both the repo and the tarball
assets="$(cd "$(dirname "$0")" && pwd)"
if [[ -x "$assets/drift" ]]; then
  default_binary="$assets/drift"
else
  default_binary="$(cd "$assets/../.." && pwd)/target/release/drift"
fi
bin_dir="$prefix/bin"
app_dir="$prefix/share/applications"
icon_dir="$prefix/share/icons/hicolor/scalable/apps"
png_dir="$prefix/share/icons/hicolor/256x256/apps"
id="dev.drift.Drift"

# Only refresh caches that already exist. Creating one in a user dir freezes that dir's
# contents for GTK, icons other apps install later would then go missing.
refresh() {
  if [[ -f "$app_dir/mimeinfo.cache" ]] && command -v update-desktop-database >/dev/null; then
    update-desktop-database -q "$app_dir" || true
  fi
  if [[ -f "$prefix/share/icons/hicolor/icon-theme.cache" ]] && command -v gtk-update-icon-cache >/dev/null; then
    gtk-update-icon-cache -q "$prefix/share/icons/hicolor" || true
  fi
  # KDE keeps its own menu cache
  command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 >/dev/null 2>&1 || true
}

if [[ "${1:-}" == "--uninstall" ]]; then
  rm -f "$bin_dir/drift" "$app_dir/$id.desktop" "$icon_dir/$id.svg" "$png_dir/$id.png"
  refresh
  echo "Drift removed. Your profile in ${XDG_DATA_HOME:-$HOME/.local/share}/drift is untouched."
  exit 0
fi

binary="${1:-$default_binary}"
[[ -x "$binary" ]] || { echo "no executable at $binary, build first: cargo build --release" >&2; exit 1; }

mkdir -p "$bin_dir" "$app_dir" "$icon_dir" "$png_dir"
# copy then rename, same as the updater, so a running Drift never sees a half written file
install -m755 "$binary" "$bin_dir/.drift.new"
mv -f "$bin_dir/.drift.new" "$bin_dir/drift"
install -m644 "$assets/$id.svg" "$icon_dir/$id.svg"
install -m644 "$assets/$id.png" "$png_dir/$id.png"
# ~/.local/bin is not on PATH in every session, so point the launcher at the absolute path
sed "s|^Exec=drift|Exec=$bin_dir/drift|" "$assets/$id.desktop" > "$app_dir/$id.desktop"
chmod 644 "$app_dir/$id.desktop"
refresh

echo "Drift $("$bin_dir/drift" --version | cut -d' ' -f2) installed to $bin_dir/drift"
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "note: $bin_dir is not on your PATH, the app menu entry works regardless" ;;
esac
