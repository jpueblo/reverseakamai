#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
package=workspace
profile=(--release)
offline=()
run_tests=false
while (($#)); do
    case "$1" in
        --package)
            [[ $# -ge 2 ]] || { echo '--package needs a value' >&2; exit 2; }
            package="$2"; shift 2 ;;
        --debug) profile=(); shift ;;
        --offline) offline=(--offline); shift ;;
        --test) run_tests=true; shift ;;
        *) echo "Unknown option: $1" >&2; exit 2 ;;
    esac
done
case "$package" in
    workspace) selection=(--workspace) ;;
    codec) selection=(-p akamai-codec) ;;
    sdk) selection=(-p akamai-solver) ;;
    *) echo 'Choose --package workspace, codec, or sdk' >&2; exit 2 ;;
esac
[[ -f /etc/os-release ]] || { echo 'Ubuntu 24.04 is required' >&2; exit 2; }
# shellcheck source=/dev/null
source /etc/os-release
[[ "$ID" == ubuntu && "$VERSION_ID" == 24.04 && "$(uname -m)" == x86_64 ]] || {
    echo 'This build entry point supports Ubuntu 24.04 x64 only' >&2; exit 2;
}
tools=(cargo rustc)
if [[ "$package" != codec ]]; then
    tools+=(cmake ninja clang perl git pkg-config)
    export CMAKE_GENERATOR=Ninja
    if [[ -z "${LIBCLANG_PATH:-}" ]] && command -v llvm-config >/dev/null; then
        LIBCLANG_PATH="$(llvm-config --libdir)"
        export LIBCLANG_PATH
    fi
fi
for tool in "${tools[@]}"; do
    command -v "$tool" >/dev/null || { echo "Missing prerequisite: $tool" >&2; exit 2; }
done
cd "$root"
common=(--locked --target x86_64-unknown-linux-gnu "${selection[@]}" "${profile[@]}" "${offline[@]}")
echo "Building $package for x86_64-unknown-linux-gnu from $root"
cargo build "${common[@]}" --all-targets
if "$run_tests"; then
    cargo test "${common[@]}" --lib --bins
fi
