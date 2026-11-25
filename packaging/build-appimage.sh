#!/bin/sh
# Builds dist/CNC_Anything-x86_64.AppImage: the release program, its icon and desktop entry,
# packed into a squashfs image behind the official AppImage type 2 runtime.
# Needs: cargo, mksquashfs (squashfs-tools), and curl the first time (to fetch the runtime).
set -eu
cd "$(dirname "$0")/.."
ROOT=$(pwd)
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
RUNTIME=packaging/runtime-x86_64
RUNTIME_URL=https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64

cargo build --release

if [ ! -s "$RUNTIME" ]; then
    echo "Downloading the AppImage runtime..."
    curl -sSfL -o "$RUNTIME" "$RUNTIME_URL"
fi

# a fresh AppDir each time (a new folder name, so nothing old is reused)
APPDIR=$(mktemp -d "$ROOT/target/CNC_Anything.AppDir.XXXXXX")
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/256x256/apps" "$APPDIR/usr/share/metainfo" "$APPDIR/usr/share/doc/cnc-anything"
cp target/release/cnc-anything "$APPDIR/usr/bin/cnc-anything"
target/release/cnc-anything --write-icon "$APPDIR/usr/share/icons/hicolor/256x256/apps/cnc-anything.png"
cp "$APPDIR/usr/share/icons/hicolor/256x256/apps/cnc-anything.png" "$APPDIR/cnc-anything.png"
ln -s cnc-anything.png "$APPDIR/.DirIcon"
cp assets/fonts/NOTO-COPYRIGHT.txt "$APPDIR/usr/share/doc/cnc-anything/"

cat > "$APPDIR/usr/share/applications/cnc-anything.desktop" <<EOF
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
X-AppImage-Version=$VERSION
EOF
cp "$APPDIR/usr/share/applications/cnc-anything.desktop" "$APPDIR/cnc-anything.desktop"

cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/cnc-anything" "$@"
EOF
chmod +x "$APPDIR/AppRun"

mkdir -p dist
SQUASH="$ROOT/target/cnc-anything-$VERSION.squashfs"
mksquashfs "$APPDIR" "$SQUASH" -root-owned -noappend -comp gzip -quiet >/dev/null
OUT="dist/CNC_Anything-$VERSION-x86_64.AppImage"
cat "$RUNTIME" "$SQUASH" > "$OUT"
chmod +x "$OUT"
cp "$OUT" dist/CNC_Anything-x86_64.AppImage
echo "Built $ROOT/$OUT ($(du -h "$OUT" | cut -f1))"
