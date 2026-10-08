#!/usr/bin/env bash
# Run every test in the repository: hook-kit, the three templates, and the starter.
#
#   scripts/test.sh          in-process tests (the hook runs natively)
#   scripts/test.sh --sbf    also build each program for SBF and run its tests against the real binary
#
# To test just one hook while you work on it: `cargo test` inside its directory.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo test --workspace
(cd starter && cargo test)

if [ "${1:-}" = "--sbf" ]; then
  for dir in templates/fair-launch templates/creator-commitment templates/holder-rewards starter; do
    scripts/build.sh "$dir"
    # ProgramTest prefers a .so found in SBF_OUT_DIR over the native program.
    (cd "$dir" && SBF_OUT_DIR="$PWD/target/deploy" cargo test)
  done
fi
