#!/bin/sh
# Builds every package from the current commit: the AppImage and the x86_64 .deb.
# Commit first: the program shows the commit id it was built from in its title bar.
set -eu
cd "$(dirname "$0")"
sh ./build-appimage.sh
sh ./build-deb.sh
