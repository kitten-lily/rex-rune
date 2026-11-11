#!/usr/bin/env bash
# Checks that the "Includes local Dracula Pro themes" marker survives a stripped release build
# made against the synthetic fixture, and is absent from a build with Dracula Pro disabled.
# Does two release builds into target/dracula-pro-marker. Run from the repository root.
set -euo pipefail

marker="Includes local Dracula Pro themes"
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
export CARGO_TARGET_DIR="$root/target/dracula-pro-marker"
command -v strip >/dev/null || { echo "FAIL: strip is not installed" >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Prints the marker's line count in a stripped copy of the release binary. Exits the script
# (not just the substitution) on any failure: it is called as `count` with the result read
# from $count_result, because $(...) would not inherit set -e.
count() {
    rm -f "$work/rex-linux"
    strip -o "$work/rex-linux" "$CARGO_TARGET_DIR/release/rex-linux" \
        || { echo "FAIL: strip failed" >&2; exit 1; }
    [ -s "$work/rex-linux" ] || { echo "FAIL: no stripped binary" >&2; exit 1; }
    local status=0
    count_result=$(grep -c -a "$marker" "$work/rex-linux") || status=$?
    # grep exits 1 for zero matches (and prints 0); 2 is an error.
    [ "$status" -le 1 ] || { echo "FAIL: grep failed" >&2; exit 1; }
    case "$count_result" in
        '' | *[!0-9]*) echo "FAIL: bad count '$count_result'" >&2; exit 1 ;;
    esac
}

REX_DRACULA_PRO_DIR="$root/tests/fixtures/dracula-pro" \
    cargo build --locked --release --bin rex-linux
count
with=$count_result
if [ "$with" -lt 1 ]; then
    echo "FAIL: marker missing from the fixture build" >&2
    exit 1
fi

REX_DRACULA_PRO=0 cargo build --locked --release --bin rex-linux
count
without=$count_result
if [ "$without" -ne 0 ]; then
    echo "FAIL: marker present ($without) in the disabled build" >&2
    exit 1
fi
echo "ok: marker in $with line(s) with fixture, 0 when disabled"
