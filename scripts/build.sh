#!/usr/bin/env bash
# Build a hook program for Solana (SBF).
#
#   scripts/build.sh HOOK_DIR        e.g. scripts/build.sh starter
#
# Writes HOOK_DIR/target/deploy/<name>.so and, the first time, <name>-keypair.json (the program
# id you will deploy under). Needs the Solana CLI tools (`cargo build-sbf`).
set -euo pipefail

dir="${1:?usage: scripts/build.sh HOOK_DIR}"
[ -f "$dir/Cargo.toml" ] || { echo "error: $dir/Cargo.toml not found" >&2; exit 1; }
command -v cargo-build-sbf >/dev/null 2>&1 || {
  echo "error: cargo build-sbf not found. Install the Solana CLI tools: https://solana.com/docs/intro/installation" >&2
  exit 1
}

out="$dir/target/deploy"
mkdir -p "$out"
cargo build-sbf --manifest-path "$dir/Cargo.toml" --sbf-out-dir "$out"

shopt -s nullglob
so_files=("$out"/*.so)
[ "${#so_files[@]}" -eq 1 ] || { echo "error: expected one .so in $out, found ${#so_files[@]}" >&2; exit 1; }
echo
echo "built ${so_files[0]} ($(wc -c <"${so_files[0]}" | tr -d ' ') bytes)"
