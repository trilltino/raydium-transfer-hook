#!/usr/bin/env bash
# Build a hook, deploy it, and (for hooks that ship a setup example) prove it on the cluster.
#
#   scripts/deploy.sh HOOK_DIR [--cluster devnet|localnet|mainnet-beta|URL] [--keypair FILE] [--limit N]
#
#   scripts/deploy.sh starter                       # devnet, ~/.config/solana/id.json
#   scripts/deploy.sh my-hook --keypair ./deployer.json --limit 1000
#
# Steps: check prerequisites, pick the cluster, check the deployer's balance, build (SBF), deploy,
# print the program id, then run HOOK_DIR/examples/devnet.rs if there is one: it creates a
# Token-2022 mint with the Transfer Hook extension, initialises the hook for that mint, and runs
# one transfer that must pass and one that must be refused.
set -euo pipefail

dir="${1:?usage: scripts/deploy.sh HOOK_DIR [--cluster devnet|localnet|mainnet-beta|URL] [--keypair FILE] [--limit N]}"
shift
cluster="devnet"
keypair="${SOLANA_KEYPAIR:-$HOME/.config/solana/id.json}"
limit="500"
while [ $# -gt 0 ]; do
  case "$1" in
    --cluster) cluster="$2"; shift 2 ;;
    --keypair) keypair="$2"; shift 2 ;;
    --limit) limit="$2"; shift 2 ;;
    *) echo "error: unknown option $1" >&2; exit 1 ;;
  esac
done

# 1. prerequisites
for tool in solana cargo cargo-build-sbf; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "error: $tool not found. Install Rust and the Solana CLI tools: https://solana.com/docs/intro/installation" >&2
    exit 1
  }
done
[ -f "$keypair" ] || {
  echo "error: deployer keypair $keypair not found. Create one with: solana-keygen new -o $keypair" >&2
  exit 1
}

# 2. cluster
case "$cluster" in
  devnet) url="https://api.devnet.solana.com" ;;
  mainnet-beta) url="https://api.mainnet-beta.solana.com" ;;
  localnet) url="http://127.0.0.1:8899" ;;
  http*) url="$cluster" ;;
  *) echo "error: unknown cluster $cluster" >&2; exit 1 ;;
esac
keypair="$(cd "$(dirname "$keypair")" && pwd)/$(basename "$keypair")"
deployer="$(solana address --keypair "$keypair")"
echo "cluster   $url"
echo "deployer  $deployer"

# 3. fund the deployer (a program costs about 5 SOL per MB in refundable rent)
balance="$(solana balance --url "$url" --keypair "$keypair" | awk '{print $1}')"
echo "balance   $balance SOL"
if awk "BEGIN{exit !($balance < 2)}"; then
  if [ "$cluster" = "devnet" ] || [ "$cluster" = "localnet" ]; then
    echo "balance is low; asking the faucet for 2 SOL (devnet's faucet is rate limited; https://faucet.solana.com works too)"
    solana airdrop 2 --url "$url" --keypair "$keypair" || echo "warning: airdrop failed; fund $deployer yourself" >&2
  else
    echo "warning: balance is under 2 SOL; the deploy may fail" >&2
  fi
fi

# 4. build
[ -f "$dir/Cargo.toml" ] || { echo "error: $dir/Cargo.toml not found" >&2; exit 1; }
cargo build-sbf --manifest-path "$dir/Cargo.toml" --sbf-out-dir "$dir/target/deploy"
so="$(ls "$dir"/target/deploy/*.so)"
[ "$(echo "$so" | wc -l)" -eq 1 ] || { echo "error: expected one .so in $dir/target/deploy" >&2; exit 1; }
echo "built $so ($(wc -c <"$so" | tr -d " ") bytes)"
program_keypair="${so%.so}-keypair.json"
program_id="$(solana address --keypair "$program_keypair")"

# 5. deploy (re-running upgrades the same program id)
solana program deploy "$so" --program-id "$program_keypair" \
  --url "$url" --keypair "$keypair" --upgrade-authority "$keypair"
echo
echo "hook program id   $program_id"
echo "upgrade authority $deployer   (it can replace your rule for every mint; revoke it with: solana program set-upgrade-authority $program_id --final)"

# 6. mint, hook setup and a test transfer, for hooks that ship the example
if [ -f "$dir/examples/devnet.rs" ]; then
  echo
  (cd "$dir" && cargo run --quiet --example devnet -- \
    --url "$url" --keypair "$keypair" --program-id "$program_id" --limit "$limit")
else
  echo
  echo "$dir has no examples/devnet.rs. Create a Token-2022 mint with the Transfer Hook extension pointing at"
  echo "$program_id and initialise the hook for it as the hook's README describes."
fi
