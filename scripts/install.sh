#!/bin/sh
# Install the sigy binary from this checkout. This is not cargo verify.
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$root"

if [ -x "$HOME/.cargo/bin/cargo" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
    export PATH
fi

if ! command -v cargo >/dev/null 2>&1; then
    if ! command -v curl >/dev/null 2>&1; then
        echo "cargo is missing and curl is unavailable. Install Rust 1.98.1, then run this script again." >&2
        exit 1
    fi
    echo "Installing Rust 1.98.1 for the current user."
    installer=$(mktemp)
    curl --proto '=https' --tlsv1.2 -fsS https://sh.rustup.rs -o "$installer"
    sh "$installer" -y --default-toolchain 1.98.1 --profile minimal
    rm -f "$installer"
    . "$HOME/.cargo/env"
fi

cargo install --path crates/sigy --locked --force
echo "Installed sigy. Next: sigy --help"
echo "Create a private library with: sigy --data-dir PATH_TO_LIBRARY library init"
if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "Recording and playback need a trusted FFmpeg. This script does not download it."
fi
