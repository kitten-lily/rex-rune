#!/usr/bin/env bash
# Install or remove Rex Linux for the current user.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
action="${1:-install}"
binary_dir="${XDG_BIN_HOME:-$HOME/.local/bin}"
applications="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
icons="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
binary="$binary_dir/rex-linux"
desktop="$applications/dev.shuv.rex-linux.desktop"

refresh_caches() {
    if command -v gtk-update-icon-cache >/dev/null && [[ -d "$icons" ]]; then
        gtk-update-icon-cache -f -t "$icons" >/dev/null 2>&1 || true
    fi
    if command -v update-desktop-database >/dev/null && [[ -d "$applications" ]]; then
        update-desktop-database "$applications"
    fi
}

desktop_exec_quote() {
    local value="$1"
    if [[ "$value" == *$'\n'* ]]; then
        echo "Desktop launcher values cannot contain newlines" >&2
        return 1
    fi
    value="${value//\\/\\\\}"
    value="${value//\"/\\\"}"
    value="${value//\`/\\\`}"
    value="${value//\$/\\\$}"
    printf '"%s"' "$value"
}

case "$action" in
    install)
        source_binary="$(realpath -- "${2:-$root/target/release/rex-linux}")"
        if [[ ! -x "$source_binary" ]]; then
            echo "Build Rex Linux first: cargo build --locked --release --bin rex-linux" >&2
            exit 1
        fi

        font_family="${REX_INSTALL_FONT:-}"
        exec_value="$(desktop_exec_quote "$binary")"
        if [[ -n "$font_family" ]]; then
            exec_value+=" --font $(desktop_exec_quote "$font_family")"
        fi

        install -Dm755 "$source_binary" "$binary"
        for size in 32 128 256; do
            install -Dm644 "$root/resources/icons/${size}x${size}/apps/dev.shuv.rex-linux.png" \
                "$icons/${size}x${size}/apps/dev.shuv.rex-linux.png"
        done
        desktop-file-install --dir="$applications" \
            --set-key=Exec --set-value="$exec_value" \
            --set-key=TryExec --set-value="$binary" \
            "$root/resources/dev.shuv.rex-linux.desktop"
        desktop-file-validate "$desktop"
        refresh_caches
        echo "Installed $binary and $desktop"
        ;;
    uninstall)
        rm -f -- \
            "$binary" \
            "$desktop" \
            "$icons/32x32/apps/dev.shuv.rex-linux.png" \
            "$icons/128x128/apps/dev.shuv.rex-linux.png" \
            "$icons/256x256/apps/dev.shuv.rex-linux.png"
        refresh_caches
        echo "Removed Rex Linux from the current user"
        ;;
    *)
        echo "Usage: $0 {install [path/to/rex-linux]|uninstall}" >&2
        exit 2
        ;;
esac
