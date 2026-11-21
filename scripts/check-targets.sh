#!/usr/bin/env bash
# Check the workspace for every target we ship, in one command.
#
# `cargo check` does not link, so this needs no cross linker, no SDK and no Docker: the windows
# target is checked without xwin, the linux target without a gcc for it. That is enough to catch
# the thing this exists for, code that compiles only on the machine it was written on.

set -euo pipefail

cd "$(dirname "$0")/.."

targets=(
    aarch64-apple-darwin
    x86_64-unknown-linux-gnu
    x86_64-pc-windows-msvc
)

failed=()

for target in "${targets[@]}"; do
    printf '\n=== %s\n' "$target"
    if cargo check --workspace --all-targets --target "$target"; then
        printf '    ok\n'
    else
        failed+=("$target")
    fi
done

if [ ${#failed[@]} -ne 0 ]; then
    printf '\nthese targets do not compile: %s\n' "${failed[*]}" >&2
    exit 1
fi

printf '\nall %d targets compile\n' "${#targets[@]}"
