# Shared by the packaging scripts (sourced, not run on its own).
# Finds the Rust toolchain and checks the other tools before anything is built.

# Rust installed with rustup lives in ~/.cargo/bin, which a plain `sh` doesn't put on PATH.
find_cargo() {
    if command -v cargo >/dev/null 2>&1; then
        return 0
    fi
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
    if command -v cargo >/dev/null 2>&1; then
        return 0
    fi

    echo "Rust's build tool (cargo) wasn't found." >&2
    if [ "${INSTALL_RUST:-}" = "1" ] || { [ -t 0 ] && printf 'Install Rust now from rustup.rs for this user (no admin rights needed)? [y/N] ' >&2 && read -r answer && [ "$answer" = "y" -o "$answer" = "Y" ]; }; then
        if ! command -v curl >/dev/null 2>&1; then
            echo "curl is needed to download the Rust installer. Install curl, then run this again." >&2
            exit 1
        fi
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
        # shellcheck disable=SC1091
        . "$HOME/.cargo/env"
        command -v cargo >/dev/null 2>&1 && return 0
    fi
    cat >&2 <<'MSG'

To build CNC Anything you need the Rust toolchain:
  1. Install it from https://rustup.rs (it installs for your user only), or run this
     script again with INSTALL_RUST=1 to have it install Rust for you.
  2. Open a new terminal (or run:  . "$HOME/.cargo/env"), then run this script again.
MSG
    exit 1
}

# need_tool <command> <what it's for> <Debian/Ubuntu package>
need_tool() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "'$1' wasn't found: it's needed $2." >&2
        echo "On Debian or Ubuntu it comes in the '$3' package (install it with your package manager)." >&2
        exit 1
    fi
}

# The C compiler and linker Rust uses on Linux
need_linker() {
    if ! command -v cc >/dev/null 2>&1 && ! command -v gcc >/dev/null 2>&1 && ! command -v clang >/dev/null 2>&1; then
        echo "No C compiler/linker (cc) was found: Rust needs one to link the program." >&2
        echo "On Debian or Ubuntu it comes in the 'build-essential' package (install it with your package manager)." >&2
        exit 1
    fi
}
