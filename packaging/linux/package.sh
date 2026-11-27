#!/usr/bin/env bash
# Builds dist/drift-<version>-x86_64-linux.tar.gz and dist/Drift-<version>-x86_64.AppImage
# from an existing release build. Run from the repo root after cargo build --release.
set -euo pipefail

version="${1:?usage: package.sh <version>}"
arch="x86_64"
root="$(pwd)"
bin="$root/target/release/drift"
dist="$root/dist"

[[ -x "$bin" ]] || { echo "missing $bin, run cargo build --release first" >&2; exit 1; }
rm -rf "$dist" && mkdir -p "$dist"

# tarball: plain binary plus desktop integration files
tardir="$dist/drift-$version-$arch-linux"
mkdir -p "$tardir"
install -m755 "$bin" "$tardir/drift"
install -m644 packaging/linux/dev.drift.Drift.desktop "$tardir/"
install -m644 packaging/linux/dev.drift.Drift.svg packaging/linux/dev.drift.Drift.png "$tardir/"
install -m755 packaging/linux/install.sh "$tardir/"
install -m644 LICENSE README.md "$tardir/"
tar -C "$dist" -czf "$tardir.tar.gz" "$(basename "$tardir")"
rm -rf "$tardir"

# AppImage
appdir="$dist/Drift.AppDir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/scalable/apps" "$appdir/usr/share/icons/hicolor/256x256/apps"
install -m755 "$bin" "$appdir/usr/bin/drift"
install -m644 packaging/linux/dev.drift.Drift.desktop "$appdir/usr/share/applications/"
install -m644 packaging/linux/dev.drift.Drift.svg "$appdir/usr/share/icons/hicolor/scalable/apps/"
install -m644 packaging/linux/dev.drift.Drift.png "$appdir/usr/share/icons/hicolor/256x256/apps/"
cp packaging/linux/dev.drift.Drift.desktop packaging/linux/dev.drift.Drift.svg "$appdir/"
ln -s usr/bin/drift "$appdir/AppRun"

tool="$dist/appimagetool"
curl -fsSL -o "$tool" \
  "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$arch.AppImage"
chmod +x "$tool"
ARCH="$arch" "$tool" --no-appstream "$appdir" "$dist/Drift-$version-$arch.AppImage"
rm -rf "$appdir" "$tool"

(cd "$dist" && sha256sum ./*.tar.gz ./*.AppImage > SHA256SUMS)
ls -lh "$dist"
