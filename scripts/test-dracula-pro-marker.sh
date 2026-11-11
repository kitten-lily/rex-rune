#!/usr/bin/env bash
# Checks that the "Includes local Dracula Pro themes" marker survives a stripped release build
# made against the synthetic fixture, and is absent from a build with Dracula Pro disabled.
# Does two release builds into target/dracula-pro-marker. Run from the repository root.
set -euo pipefail

marker="Includes local Dracula Pro themes"
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
export CARGO_TARGET_DIR="$root/target/dracula-pro-marker"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

count() {
    strip -o "$work/rex-linux" "$CARGO_TARGET_DIR/release/rex-linux"
    grep -c -a "$marker" "$work/rex-linux" || true
}

REX_DRACULA_PRO_DIR="$root/tests/fixtures/dracula-pro" \
    cargo build --locked --release --bin rex-linux
with=$(count)
if [ "$with" -lt 1 ]; then
    echo "FAIL: marker missing from the fixture build" >&2
    exit 1
fi

REX_DRACULA_PRO=0 cargo build --locked --release --bin rex-linux
without=$(count)
if [ "$without" -ne 0 ]; then
    echo "FAIL: marker present ($without) in the disabled build" >&2
    exit 1
fi
echo "ok: marker in $with line(s) with fixture, 0 when disabled"
