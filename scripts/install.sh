#!/bin/sh
# Install sigy from https://github.com/blisspixel/sigy. This is not cargo verify.
(
set -eu
starting_directory=$(pwd)
if [ -n "${CARGO_HOME:-}" ]; then
    case "$CARGO_HOME" in /*) ;; *) CARGO_HOME="$starting_directory/$CARGO_HOME";; esac
    export CARGO_HOME
fi
if [ -n "${CARGO_INSTALL_ROOT:-}" ]; then
    case "$CARGO_INSTALL_ROOT" in /*) ;; *) CARGO_INSTALL_ROOT="$starting_directory/$CARGO_INSTALL_ROOT";; esac
    export CARGO_INSTALL_ROOT
fi

repo=https://github.com/blisspixel/sigy.git
export GIT_TERMINAL_PROMPT=0
export GIT_CONFIG_NOSYSTEM=1
export GIT_CONFIG_GLOBAL=/dev/null
unset GIT_CONFIG_PARAMETERS GIT_CONFIG_COUNT GIT_SSH_COMMAND GIT_ASKPASS SSH_ASKPASS GIT_EXEC_PATH 2>/dev/null || true

sigy_git() {
    if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
        git -c core.abbrev=40 -c core.fsmonitor= -c core.hooksPath=/dev/null -c http.followRedirects=false -c protocol.version=2 -c transfer.fsckObjects=true -c credential.helper= -c 'credential.helper=!gh auth git-credential' "$@"
    else
        git -c core.abbrev=40 -c core.fsmonitor= -c core.hooksPath=/dev/null -c http.followRedirects=false -c protocol.version=2 -c transfer.fsckObjects=true -c credential.helper= "$@"
    fi
}

sigy_check_managed_source() {
    origin=$(sigy_git -C "$root" config --local --get remote.origin.url) || {
        echo "Managed sigy source has no readable origin." >&2
        exit 1
    }
    if [ "$origin" != "$repo" ]; then
        echo "Managed sigy source origin is not $repo." >&2
        exit 1
    fi
    dirty=$(sigy_git -C "$root" status --porcelain=v1 --untracked-files=all) || {
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

if [ "$from_github" -eq 1 ] && [ -z "${SIGY_NO_FETCH:-}" ]; then
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
    sigy_git -C "$root" fetch --depth 1 "$repo" main
    sigy_git -C "$root" checkout --detach FETCH_HEAD
    sigy_check_managed_source
fi

cd "$root"

cargo_home=${CARGO_HOME:-$HOME/.cargo}
if [ -x "$cargo_home/bin/cargo" ]; then
    PATH="$cargo_home/bin:$PATH"
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
    . "$cargo_home/env"
fi

unset SIGY_GIT_COMMIT
commit=""
if sigy_git -C "$root" rev-parse HEAD >/dev/null 2>&1; then
    local_changes=$(sigy_git -C "$root" status --porcelain=v1 --untracked-files=all) || local_changes=unknown
    if [ -z "$local_changes" ]; then
        commit=$(sigy_git -C "$root" rev-parse HEAD)
        SIGY_GIT_COMMIT=$commit
        export SIGY_GIT_COMMIT
    fi
fi

stage_dir=$(mktemp -d 2>/dev/null || mktemp -d -t 'sigy-stage')
cleanup_stage() {
    rm -rf "$stage_dir"
}
trap cleanup_stage EXIT INT TERM

if [ -n "$commit" ]; then
    sigy_git -C "$root" archive "$commit" | (cd "$stage_dir" && tar -x) || {
        echo "Failed to extract git archive for $commit" >&2
        exit 1
    }
    if [ ! -f "$stage_dir/crates/sigy/Cargo.toml" ]; then
        echo "Staged archive does not contain a valid sigy tree" >&2
        exit 1
    fi
else
    cp -R "$root/." "$stage_dir/"
fi

(cd "$stage_dir" && cargo install --path crates/sigy --locked --force)
if [ -n "$commit" ]; then
    after_commit=$(sigy_git -C "$root" rev-parse HEAD)
    after_changes=$(sigy_git -C "$root" status --porcelain=v1 --untracked-files=all)
    if [ "$after_commit" != "$commit" ] || [ -n "$after_changes" ]; then
        echo "Source changed during installation; installation identity is unproven." >&2
        exit 1
    fi
fi

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
echo "Start with: sigy init --radio"
echo "Open the explorer with: sigy tui"
if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "Recording and playback need a trusted FFmpeg. This script does not download it."
fi
)
