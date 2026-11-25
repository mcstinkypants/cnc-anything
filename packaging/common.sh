# Shared by the packaging scripts (sourced, not run on its own).
# Checks everything a build needs before anything is built, and explains what's missing.
#
#   check_prereqs appimage|deb|all
#
# What building CNC Anything needs
#   Rust toolchain (cargo, rustc)  the language CNC Anything is written in; turns the source into the program
#   C compiler / linker (cc)       Rust uses it to link the final program
#   git                            stamps the commit id shown in the app's title bar (optional)
#   AppImage:  mksquashfs (squashfs-tools), curl (first build only, fetches the AppImage runtime)
#   .deb:      dpkg-deb (dpkg), objdump (binutils)

RUSTUP_URL="https://rustup.rs"

# Rust installed with rustup lives in ~/.cargo/bin, which a plain `sh` doesn't put on PATH.
load_cargo() {
    command -v cargo >/dev/null 2>&1 && return 0
    if [ -f "$HOME/.cargo/env" ]; then
        # shellcheck disable=SC1091
        . "$HOME/.cargo/env"
    fi
    for d in "${CARGO_HOME:-}/bin" "$HOME/.cargo/bin" /usr/local/cargo/bin; do
        if [ -n "$d" ] && [ -x "$d/cargo" ]; then
            PATH="$d:$PATH"
            export PATH
            break
        fi
    done
    command -v cargo >/dev/null 2>&1
}

have() { command -v "$1" >/dev/null 2>&1; }
have_cc() { have cc || have gcc || have clang; }

# one line of the checklist: mark, name, what it's for
line() {
    if [ "$1" = "ok" ]; then mark="  [found]  "; else mark="  [MISSING]"; fi
    printf '%s %-24s %s\n' "$mark" "$2" "$3" >&2
}

# install hints for the missing system packages, per Linux family
install_hint() {
    pkgs_deb="$1"; pkgs_rpm="$2"; pkgs_arch="$3"
    [ -n "$pkgs_deb" ] || return 0
    echo "" >&2
    echo "Install the missing system tools with your package manager, for example:" >&2
    echo "  Debian / Ubuntu / Mint:   sudo apt install $pkgs_deb" >&2
    echo "  Fedora / RHEL:            sudo dnf install $pkgs_rpm" >&2
    echo "  Arch / Manjaro:           sudo pacman -S $pkgs_arch" >&2
}

check_prereqs() {
    what="${1:-all}"
    rust_ok=no; load_cargo && rust_ok=ok
    cc_ok=no; have_cc && cc_ok=ok
    git_ok=no; have git && git_ok=ok
    missing=""; deb=""; rpm=""; arch=""
    add() { deb="$deb $1"; rpm="$rpm $2"; arch="$arch $3"; }

    [ "$cc_ok" = ok ] || { missing=1; add build-essential "gcc" base-devel; }
    if [ "$what" = appimage ] || [ "$what" = all ]; then
        sq_ok=no; have mksquashfs && sq_ok=ok
        curl_ok=ok
        if [ ! -s packaging/runtime-x86_64 ] && ! have curl; then curl_ok=no; fi
        [ "$sq_ok" = ok ] || { missing=1; add squashfs-tools squashfs-tools squashfs-tools; }
        [ "$curl_ok" = ok ] || { missing=1; add curl curl curl; }
    fi
    if [ "$what" = deb ] || [ "$what" = all ]; then
        dpkg_ok=no; have dpkg-deb && dpkg_ok=ok
        obj_ok=no; have objdump && obj_ok=ok
        [ "$dpkg_ok" = ok ] || { missing=1; add dpkg dpkg dpkg; }
        [ "$obj_ok" = ok ] || { missing=1; add binutils binutils binutils; }
    fi
    [ "$rust_ok" = ok ] || missing=1

    if [ -z "$missing" ]; then
        [ "$git_ok" = ok ] || echo "Note: git wasn't found, so the title bar will show 'unknown' instead of the commit id." >&2
        return 0
    fi

    cat >&2 <<'HEAD'

==================================================================
 CNC Anything can't be built yet: some tools it needs are missing
==================================================================
CNC Anything is written in Rust, so building it from source needs the
Rust toolchain plus a few common system tools. Here's what was checked:

HEAD
    line "$rust_ok" "Rust (cargo, rustc)" "turns the source code into the program"
    line "$cc_ok" "C compiler/linker (cc)" "Rust uses it to link the program"
    if [ "$git_ok" = ok ]; then line ok "git" "puts the commit id in the title bar"; else line no "git (optional)" "puts the commit id in the title bar"; fi
    if [ "$what" = appimage ] || [ "$what" = all ]; then
        line "$sq_ok" "mksquashfs" "packs the AppImage (squashfs-tools)"
        line "$curl_ok" "curl" "fetches the AppImage runtime once"
    fi
    if [ "$what" = deb ] || [ "$what" = all ]; then
        line "$dpkg_ok" "dpkg-deb" "builds the .deb package (dpkg)"
        line "$obj_ok" "objdump" "reads the program's library needs (binutils)"
    fi

    if [ "$rust_ok" != ok ]; then
        cat >&2 <<MSG

Rust is installed per user with rustup (no administrator rights needed):
  1. Go to $RUSTUP_URL and follow the one-line install, or let this script
     do it: run it again with INSTALL_RUST=1 in front, for example
         INSTALL_RUST=1 sh packaging/release.sh
  2. Open a new terminal (or run:  . "\$HOME/.cargo/env"), then build again.
MSG
    fi
    install_hint "$(echo $deb)" "$(echo $rpm)" "$(echo $arch)"
    echo "" >&2
    echo "Check again any time with:  sh packaging/check-prereqs.sh" >&2

    # Offer to install Rust when it's the only thing missing in the way
    if [ "$rust_ok" != ok ] && have curl; then
        if [ "${INSTALL_RUST:-}" = "1" ] || { [ -t 0 ] && printf '\nInstall Rust now for this user from %s? [y/N] ' "$RUSTUP_URL" >&2 && read -r answer && { [ "$answer" = "y" ] || [ "$answer" = "Y" ]; }; }; then
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
            # shellcheck disable=SC1091
            . "$HOME/.cargo/env"
            if load_cargo && [ -n "$(echo $deb)" ]; then
                echo "Rust is installed. Install the system tools listed above, then build again." >&2
                exit 1
            fi
            load_cargo && return 0
        fi
    fi
    exit 1
}
