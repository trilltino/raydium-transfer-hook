#!/usr/bin/env bash
# Build a hook for Solana, deploy it, and, if the hook ships examples/devnet.rs, prove it on the
# cluster. Run with --help for the options.
set -euo pipefail

MAINNET_GENESIS="5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d"

usage() {
  cat <<'USAGE'
Build, deploy and (where the hook ships an example) prove a Transfer Hook on a cluster.

usage: scripts/deploy.sh HOOK_DIR [--cluster CLUSTER] [--keypair FILE] [--allow-mainnet] [-- EXAMPLE_ARGS...]

  HOOK_DIR          a hook crate, e.g. starter, my-hook, templates/fair-launch
  --cluster C       devnet (default) | localnet | mainnet-beta | an RPC URL
  --keypair FILE    deployer keypair (default: $SOLANA_KEYPAIR, else ~/.config/solana/id.json)
  --allow-mainnet   required to deploy to mainnet-beta (by name or by an RPC URL that serves it)
  -h, --help        this help
  -- ARGS           passed unchanged to HOOK_DIR/examples/devnet.rs, after --url, --keypair, --program-id

Every hook:   check tools and keypair, pick the cluster, check the balance (faucet on devnet/localnet),
              cargo build-sbf into HOOK_DIR/target/deploy, deploy under the generated program id.
Hooks with examples/devnet.rs (today: the starter): also create a hooked mint, initialise the hook
              and send one transfer that must pass and one the hook must refuse.

examples:
  scripts/deploy.sh starter                      # devnet, default keypair, the example's default limit
  scripts/deploy.sh starter -- --limit 1000      # forward an example-specific option
  scripts/deploy.sh templates/fair-launch --keypair ./devnet-deployer.json
USAGE
}

die() { echo "error: $*" >&2; exit 1; }

[ $# -gt 0 ] || { usage >&2; exit 1; }
case "$1" in -h|--help) usage; exit 0 ;; -*) die "the first argument is HOOK_DIR (see --help)" ;; esac
dir="$1"
shift
cluster="devnet"
keypair="${SOLANA_KEYPAIR:-$HOME/.config/solana/id.json}"
allow_mainnet=0
example_args=()
while [ $# -gt 0 ]; do
  case "$1" in
    --cluster) [ $# -ge 2 ] || die "--cluster needs a value"; cluster="$2"; shift 2 ;;
    --keypair) [ $# -ge 2 ] || die "--keypair needs a value"; keypair="$2"; shift 2 ;;
    --allow-mainnet) allow_mainnet=1; shift ;;
    -h|--help) usage; exit 0 ;;
    --) shift; example_args=("$@"); break ;;
    *) die "unknown option $1 (options for the hook's example go after --; see --help)" ;;
  esac
done

# 1. prerequisites
for tool in solana cargo cargo-build-sbf; do
  command -v "$tool" >/dev/null 2>&1 \
    || die "$tool not found. Install Rust and the Solana CLI tools: https://solana.com/docs/intro/installation"
done
[ -f "$keypair" ] || die "deployer keypair $keypair not found. Create a throwaway one with: solana-keygen new -o $keypair"
[ -f "$dir/Cargo.toml" ] || die "$dir/Cargo.toml not found"

# 2. cluster, with the mainnet interlock (by name, and by the genesis hash the RPC actually serves)
case "$cluster" in
  devnet) url="https://api.devnet.solana.com" ;;
  mainnet-beta) url="https://api.mainnet-beta.solana.com" ;;
  localnet) url="http://127.0.0.1:8899" ;;
  http*) url="$cluster" ;;
  *) die "unknown cluster $cluster (see --help)" ;;
esac
refuse_mainnet() {
  echo "error: refusing to deploy to mainnet-beta ($url)." >&2
  echo "Deploying spends real SOL and publishes a program whose upgrade authority is your keypair." >&2
  echo "Nothing in this repository is audited. Review the hook first, then re-run with --allow-mainnet." >&2
  exit 1
}
# By name: refuse before touching the network.
[ "$cluster" = "mainnet-beta" ] && [ "$allow_mainnet" -ne 1 ] && refuse_mainnet
# By what the RPC serves: a URL can point at mainnet under any name.
genesis="$(solana genesis-hash --url "$url" 2>/dev/null)" || die "cannot reach $url"
if [ "$genesis" = "$MAINNET_GENESIS" ]; then
  [ "$allow_mainnet" -eq 1 ] || refuse_mainnet
  echo "warning: deploying to MAINNET-BETA (--allow-mainnet given)" >&2
fi
keypair="$(cd "$(dirname "$keypair")" && pwd)/$(basename "$keypair")"
deployer="$(solana address --keypair "$keypair")"
echo "cluster   $url"
echo "deployer  $deployer"

# 3. fund the deployer (program rent is refundable; at the time of writing about 5.1 SOL per MB)
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
cargo build-sbf --manifest-path "$dir/Cargo.toml" --sbf-out-dir "$dir/target/deploy"
so="$(ls "$dir"/target/deploy/*.so)"
[ "$(echo "$so" | wc -l)" -eq 1 ] || die "expected one .so in $dir/target/deploy"
echo "built $so ($(wc -c <"$so" | tr -d " ") bytes)"
program_keypair="${so%.so}-keypair.json"
program_id="$(solana address --keypair "$program_keypair")"

# 5. deploy (re-running upgrades the same program id)
solana program deploy "$so" --program-id "$program_keypair" \
  --url "$url" --keypair "$keypair" --upgrade-authority "$keypair"
echo
echo "hook program id   $program_id"
echo "upgrade authority $deployer   (it can replace your rule for every mint; revoke it with: solana program set-upgrade-authority $program_id --final)"

# 6. mint, hook setup and a test transfer, only for hooks that ship the example
if [ -f "$dir/examples/devnet.rs" ]; then
  echo
  (cd "$dir" && cargo run --quiet --example devnet -- \
    --url "$url" --keypair "$keypair" --program-id "$program_id" \
    ${example_args[@]+"${example_args[@]}"})
else
  [ ${#example_args[@]} -eq 0 ] || echo "warning: $dir has no examples/devnet.rs; ignoring: ${example_args[*]}" >&2
  echo
  echo "Deployed only. $dir has no examples/devnet.rs, so nothing was initialised or tested on the cluster."
  echo "Next: create a Token-2022 mint whose Transfer Hook points at $program_id, then send the hook's"
  echo "initialise instruction as its README (DEPLOY / INITIALIZE) describes."
fi
