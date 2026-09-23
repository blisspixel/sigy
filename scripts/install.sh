#!/bin/sh
# Install sigy from https://github.com/blisspixel/sigy. This is not cargo verify.
set -eu

repo=https://github.com/blisspixel/sigy.git
export GIT_TERMINAL_PROMPT=0

sigy_git() {
    if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
        git -c credential.helper= -c 'credential.helper=!gh auth git-credential' "$@"
    else
        git "$@"
    fi
}

sigy_check_managed_source() {
    origin=$(git -C "$root" config --local --get remote.origin.url) || {
        echo "Managed sigy source has no readable origin." >&2
        exit 1
    }
    if [ "$origin" != "$repo" ]; then
        echo "Managed sigy source origin is not $repo." >&2
        exit 1
    fi
    dirty=$(git -C "$root" status --porcelain=v1 --untracked-files=all) || {
        echo "Cannot inspect managed sigy source changes." >&2
        exit 1
    }
    if [ -n "$dirty" ]; then
        echo "Managed sigy source has local changes. Preserve them and use a clean source directory." >&2
        exit 1
    fi
}

if [ -n "${SIGY_SRC:-}" ]; then
    root=$SIGY_SRC
    from_github=1
else
    root=""
    if [ -f "$0" ]; then
        candidate=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
        if [ -f "$candidate/crates/sigy/Cargo.toml" ]; then
            root=$candidate
        fi
    fi
    if [ -z "$root" ]; then
        root=$HOME/.sigy/src
        from_github=1
    else
        from_github=0
    fi
fi

case "$root" in
    /*) ;;
    *) root="$(pwd)/$root" ;;
esac

if [ "$from_github" -eq 1 ]; then
    if ! command -v git >/dev/null 2>&1; then
        echo "git is missing. Install Git to fetch $repo." >&2
        exit 1
    fi
    mkdir -p "$(dirname "$root")"
    if [ ! -d "$root/.git" ]; then
        if [ -e "$root" ]; then
            echo "$root exists and is not a sigy checkout." >&2
            exit 1
        fi
        sigy_git clone --depth 1 --branch main "$repo" "$root"
    fi
    sigy_check_managed_source
    sigy_git -C "$root" -c core.abbrev=40 fetch --depth 1 "$repo" main
    git -C "$root" checkout --detach FETCH_HEAD
    sigy_check_managed_source
fi

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
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
fi

unset SIGY_GIT_COMMIT
commit=""
if git -C "$root" -c core.abbrev=40 rev-parse HEAD >/dev/null 2>&1; then
    local_changes=$(git -C "$root" status --porcelain=v1 --untracked-files=all) || local_changes=unknown
    if [ -z "$local_changes" ]; then
        commit=$(git -C "$root" -c core.abbrev=40 rev-parse HEAD)
        SIGY_GIT_COMMIT=$commit
        export SIGY_GIT_COMMIT
    fi
fi

cargo install --path crates/sigy --locked --force

if [ -n "$commit" ]; then
    mkdir -p "$HOME/.sigy"
    printf '%s\n' "$commit" > "$HOME/.sigy/installed-commit"
    echo "Installed sigy at $commit."
else
    mkdir -p "$HOME/.sigy"
    printf '\n' > "$HOME/.sigy/installed-commit"
    echo "Installed sigy without a clean commit identity."
fi
echo "Check later with: sigy update --check"
echo "Install a newer main commit with: sigy update"
echo "Create a private library with: sigy --data-dir PATH_TO_LIBRARY library init"
if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "Recording and playback need a trusted FFmpeg. This script does not download it."
fi
