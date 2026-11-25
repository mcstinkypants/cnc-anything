#!/bin/sh
# Builds dist/cnc-anything_<version>_amd64.deb (x86_64) from the release program.
# Needs: cargo, dpkg-deb and objdump. Install the result with your package manager
# (for example, apt install ./dist/cnc-anything_<version>_amd64.deb).
set -eu
cd "$(dirname "$0")/.."
ROOT=$(pwd)
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
ARCH=amd64
case "$(uname -m)" in x86_64|amd64) ;; *) echo "This script packages x86_64 builds only" >&2; exit 1 ;; esac

cargo build --release
BIN=target/release/cnc-anything

# the glibc version the program needs, for the package's dependency line
GLIBC=$(objdump -T "$BIN" 2>/dev/null | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -n1 || true)
[ -n "$GLIBC" ] || GLIBC=2.31

# a fresh staging folder each time (a new name, so nothing old is reused)
STAGE=$(mktemp -d "$ROOT/target/deb.XXXXXX")
PKG="$STAGE/cnc-anything_${VERSION}_${ARCH}"
mkdir -p "$PKG/DEBIAN" "$PKG/usr/bin" "$PKG/usr/share/applications" "$PKG/usr/share/icons/hicolor/256x256/apps" "$PKG/usr/share/doc/cnc-anything"
install -m 0755 "$BIN" "$PKG/usr/bin/cnc-anything"
"$BIN" --write-icon "$PKG/usr/share/icons/hicolor/256x256/apps/cnc-anything.png"
chmod 0644 "$PKG/usr/share/icons/hicolor/256x256/apps/cnc-anything.png"

cat > "$PKG/usr/share/applications/cnc-anything.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=CNC Anything
GenericName=Laser and CNC router software
Comment=Design jobs and send them to GRBL laser cutters and CNC routers
Exec=cnc-anything %f
Icon=cnc-anything
Terminal=false
Categories=Graphics;Engineering;
MimeType=application/x-cnc-anything-project;image/svg+xml;
DESK
chmod 0644 "$PKG/usr/share/applications/cnc-anything.desktop"

{
    echo "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"
    echo "Upstream-Name: cnc-anything"
    echo
    echo "Files: *"
    echo "Copyright: the CNC Anything authors"
    echo
    echo "Files: bundled fonts (Noto Sans, Noto Serif, Noto Sans Mono)"
    echo "License: OFL-1.1"
    echo
    cat assets/fonts/NOTO-COPYRIGHT.txt
} > "$PKG/usr/share/doc/cnc-anything/copyright"
chmod 0644 "$PKG/usr/share/doc/cnc-anything/copyright"

SIZE=$(du -sk "$PKG/usr" | cut -f1)
cat > "$PKG/DEBIAN/control" <<CTRL
Package: cnc-anything
Version: $VERSION
Section: graphics
Priority: optional
Architecture: $ARCH
Installed-Size: $SIZE
Depends: libc6 (>= $GLIBC), libgcc-s1, libgl1 | libegl1, libxkbcommon0
Recommends: libx11-6, libxcursor1, libxrandr2, libxi6, libwayland-client0, libxkbcommon-x11-0
Maintainer: CNC Anything <noreply@localhost>
Description: Design jobs and send them to GRBL laser cutters and CNC routers
 Draw or import shapes, text and pictures, set up cut layers for a laser
 or 2D CNC router (tool library, speeds and feeds, depth passes, tabs),
 preview the job and stream it over USB to a GRBL controller.
CTRL

mkdir -p dist
OUT="dist/cnc-anything_${VERSION}_${ARCH}.deb"
dpkg-deb --root-owner-group --build "$PKG" "$OUT" >/dev/null
echo "Built $ROOT/$OUT ($(du -h "$OUT" | cut -f1))"
