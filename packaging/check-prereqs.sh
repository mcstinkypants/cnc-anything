#!/bin/sh
# Checks that everything needed to build CNC Anything is installed, and explains what's missing.
#   sh packaging/check-prereqs.sh            everything (AppImage and .deb)
#   sh packaging/check-prereqs.sh appimage   only what the AppImage needs
#   sh packaging/check-prereqs.sh deb        only what the .deb needs
set -eu
cd "$(dirname "$0")/.."
. packaging/common.sh
check_prereqs "${1:-all}"
echo "Everything needed to build CNC Anything is installed ($(cargo --version))."
