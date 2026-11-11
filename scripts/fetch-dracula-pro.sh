#!/usr/bin/env bash
# Fetches or updates the local checkout of the private Dracula Pro Ghostty themes
# (github.com/dracula-pro/ghostty, licensees only) that build.rs turns into optional themes.
# Uses your own git credentials and never prompts. A failure only warns: every path exits 0,
# so a build without access, or offline, goes on with what it has.
#
# REX_DRACULA_PRO=0        skip (also disables theme generation in build.rs)
# REX_DRACULA_PRO_DIR      checkout location (same default as build.rs)
# REX_DRACULA_PRO_URL      remote (default https://github.com/dracula-pro/ghostty.git)

[ "${REX_DRACULA_PRO:-}" = "0" ] && exit 0

url=${REX_DRACULA_PRO_URL:-https://github.com/dracula-pro/ghostty.git}
dir=${REX_DRACULA_PRO_DIR:-}
if [ -z "$dir" ]; then
    case "${XDG_DATA_HOME:-}" in
        /*) data=$XDG_DATA_HOME ;;
        *) data=${HOME:+$HOME/.local/share} ;;
    esac
    if [ -z "${data:-}" ]; then
        echo "warning: Dracula Pro themes not fetched (no checkout directory: HOME is unset); building with no Dracula Pro themes. Check access with: gh auth status && gh auth setup-git" >&2
        exit 0
    fi
    dir=$data/rex-linux/dracula-pro-ghostty
fi

# $1 is the reason; the build goes on with the checkout if there is one.
warn() {
    if [ -d "$dir/.git" ]; then have="the existing checkout"; else have="no Dracula Pro themes"; fi
    echo "warning: Dracula Pro themes not fetched ($1); building with $have. Check access with: gh auth status && gh auth setup-git" >&2
}

# Never put the licensed files inside a work tree that could commit them.
top=$(git rev-parse --show-toplevel 2>/dev/null)
if [ -n "$top" ]; then
    abs=$(realpath -m -- "$dir" 2>/dev/null)
    top=$(realpath -m -- "$top" 2>/dev/null)
    case "$abs/" in
        "$top"/*)
            warn "the directory is inside the git work tree $top"
            exit 0
            ;;
    esac
fi

fetch_git() {
    GIT_TERMINAL_PROMPT=0 GIT_ASKPASS= SSH_ASKPASS= GCM_INTERACTIVE=never \
        timeout 60 git -c http.lowSpeedLimit=1000 -c http.lowSpeedTime=15 "$@" >/dev/null 2>&1
}

if ! mkdir -p -- "$(dirname -- "$dir")"; then
    warn "cannot create the parent directory"
    exit 0
fi

if [ -d "$dir/.git" ]; then
    fetch_git -C "$dir" pull --ff-only --quiet "$url" main || warn "git pull failed or timed out"
elif [ -e "$dir" ]; then
    warn "the directory exists but is not a git checkout"
else
    tmp=$dir.tmp.$$
    if fetch_git clone --quiet "$url" "$tmp" && mv -- "$tmp" "$dir"; then
        :
    else
        rm -rf -- "$tmp"
        warn "git clone failed or timed out"
    fi
fi
exit 0
