# The Fair Launch reference UI

A small web app that swaps a hooked token on a hook-aware Raydium deployment and shows the hook
allow or refuse the trade. It is a reference and a pattern to copy, not the Raydium app, and it
talks only to **our** experimental deployments (localnet and integration devnet), never to
Raydium's.

```text
packages/transfer-hook-client   the hook-aware client (Apache-2.0, no Raydium SDK)
apps/fair-launch-ui             the React app (GPL-3.0-or-later, because it uses Raydium SDK V2)
```

## What it proves

* A browser can resolve each swap leg's Transfer Hook accounts (with the SPL JavaScript helper, not
  a reimplementation), build `swap_base_input_v2` or `swap_v3`, simulate it, and only then ask a
  wallet to sign.
* A buy inside the launch limits succeeds, and an over-limit buy is refused **by the hook** during the
  simulation, shown as words ("Your wallet balance would exceed the configured launch limit"), with
  nothing submitted and no balance changed.
* A sell is not treated as a buy: the Fair Launch rules bind only transfers out of a launch venue.
* The same shell works for CPMM and CLMM pools (the adapter is chosen from the pool's owner program)
  and, by swapping the policy panel, for the other examples.

The browser test in [`apps/fair-launch-ui/e2e`](../apps/fair-launch-ui/e2e) runs exactly this against a
local validator: one passing buy, one hook-refused buy, one sell.

## Run it on a local validator

```sh
npm install
cargo xtask localnet build                    # the pinned Raydium forks and this repo's hooks
cargo xtask localnet validator                # leave running; in another terminal:
cargo xtask localnet ui-fixture --wallet <YOUR_WALLET_PUBKEY> --out target/ui-e2e/fixture.json
npm run ui:dev
```

`ui-fixture` creates a Fair Launch pool (100 tokens per buy, 300 per account, three buys per slot, a
one-hour window) and gives the wallet SOL and both tokens. Open
`http://127.0.0.1:5173/?env=localnet&pool=<pool from fixture.json>` and connect Phantom or Solflare
pointed at `http://127.0.0.1:8899`. `?env=devnet` selects our integration devnet deployment.

The page lists only `localnet` and `integration-devnet`. There is no official-Raydium mode: Raydium's
own programs do not contain the hook-aware instructions.

## How the Raydium SDK is used, and why the swap is built separately

[`@raydium-io/raydium-sdk-v2`](https://www.npmjs.com/package/@raydium-io/raydium-sdk-v2) (pinned) is
used for what it is good at: reading pool and config accounts (`getRpcPoolInfo`, `getPoolInfoFromRpc`)
and the quote math (`CurveCalculator`, `PoolUtils.computeAmountOut`). Its swap builders are **not**
used: they emit the stock `swap_base_input` / `swap_v2`, which do not know the `_v2` / `swap_v3`
instructions or their hook-account counts. The client builds those:

| | |
|---|---|
| per leg | the mint's TransferHook extension decides whether there is a hook; the validation list decides the extras |
| slice | `extras…, hook program, validation list` (empty if the leg has no hook), never merged, sorted or deduplicated |
| CPMM | the 13 fixed accounts, then the input leg's slice, then the output leg's; data = V1 data with the new discriminator and two `u16` counts |
| CLMM | the 13 fixed accounts, tick arrays, the optional bitmap extension, then the two slices; four `u16` counts |
| checks | extras never sign, are writable only if the caller named them, may not escalate a fixed account; the hook program must be the expected one |

The client's output is checked byte for byte against the Rust crate's committed golden files
(`crates/transfer-hook-sdk/tests/golden`), so the two languages cannot drift apart unnoticed.

## The swap button, in order

Reload the pool and the launch state, quote, read each leg's hook, resolve both slices, build the
hook-aware instruction, compile a v0 transaction, **simulate**, and only if that passes ask the wallet to
sign, then send and confirm and refresh. The reference app uses v0 transactions because the Fair Launch
priority-fee rule reads `SetComputeUnitPrice`, which is unreliable on v1 transactions.

## Pointing it at a pool

`?pool=<address>`. The page fails closed unless the pool is owned by the environment's Raydium program,
is open for swaps and, when the pool holds a Fair Launch token, the pool's vault of that token is one of
the launch's venues. A hooked mint you have not seen before is shown in full with its hook program and
must be confirmed once (the mint address is the identity, never the symbol).

## Reading the Fair Launch panel

Each limit that is switched on is a meter (a limit of `0` is off and not shown). For a buy the meters show
what this trade would use: buy size, the wallet's balance after the buy, buys in the current slot, and the
declared priority fee. A meter turns red and a warning appears before signing when the trade would break a
rule. The warning is advisory: the Swap button stays enabled so the simulation, which runs the real hook,
has the last word. Outside the window the panel says so and shows no meters.

## Copying the pattern for another hook

Keep `packages/transfer-hook-client` and the transaction layer (`src/lib/swap.ts`, `run-swap.ts`) and
replace the policy panel (`FairLaunchPolicy`) and its decoder (`packages/transfer-hook-client/src/fair-launch`)
with your hook's state and error codes. Creator Commitment needs the creator account, the locked total and
the vesting progress; Holder Rewards adds registered / earned / claimable and Register and Claim buttons
(those are ordinary instructions of the hook program, not part of the swap).

## Limits

* Experimental deployments only; no mainnet and no official Raydium.
* CPMM and CLMM exact-input swaps. Exact-output swaps and liquidity operations are not in the UI.
* Wrapped SOL is not handled; use two SPL tokens.
* Only the fair-launch hook has a decoder. Another hook's errors are reported as "Transfer Hook rejected
  this transaction" with the raw code, never guessed.
* The test wallet used by the browser test exists only in builds made with `VITE_E2E=1`; normal builds
  contain no key handling. The app has no backend and holds no secrets.
* Licence: the client package is Apache-2.0 like the Rust crates; the app is GPL-3.0-or-later because it
  links Raydium SDK V2 (GPL-3.0). Do not copy SDK source into this repository's Rust crates.
