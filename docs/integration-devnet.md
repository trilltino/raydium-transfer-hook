# Integration devnet

Official Raydium programs, including Raydium's own devnet deployments, do not contain the hook-aware
instructions (`swap_base_input_v2`, `swap_v3`). To run a hooked swap on a real cluster before
Raydium adopts them, the hook-support forks are built with an `integration` feature and deployed
under **our own program ids**. This page records exactly what is deployed and what ran. It is a test
environment, not an official Raydium deployment, and not an audit.

## Programs

| Program | Id | Built from |
|---|---|---|
| Hook-aware CPMM | [`7tRJH4mmEfNGGLf9E8qEvo3oSjq2b75DhSggb1Wz45fJ`](https://explorer.solana.com/address/7tRJH4mmEfNGGLf9E8qEvo3oSjq2b75DhSggb1Wz45fJ?cluster=devnet) | https://github.com/trilltino/raydium-cp-swap @ 75ddc09f102c8e3e4424058cee188bf8277949fc (feature: integration; base raydium-io/raydium-cp-swap b3187ae53a1b95a201f855a59024a12ca8f5b51a) |
| Hook-aware CLMM | [`3dNJapViueBArDN3HbWKcEf2u6hfZQ3oDyh3GmUXJ8oD`](https://explorer.solana.com/address/3dNJapViueBArDN3HbWKcEf2u6hfZQ3oDyh3GmUXJ8oD?cluster=devnet) | https://github.com/trilltino/raydium-clmm @ 40291d53d84c6a28991ed966aa2efd261843f662 (feature: integration; base raydium-io/raydium-clmm ed1eb41519d5355755f7df52b43fa9610938b60b) |
| Reference hook | [`Cz3Ge1ENd1yZAbtXA88dYaxA8x4ZkSxxdHmSj7nxQ11X`](https://explorer.solana.com/address/Cz3Ge1ENd1yZAbtXA88dYaxA8x4ZkSxxdHmSj7nxQ11X?cluster=devnet) | trilltino/raydium-transfer-hook programs/reference-hook-onchain @ 9cc41da |
| Unrelated arbitrary hook | [`EtXNoNoYQdF9QFaSttjGzZkAk29EcBW9apdcE4En6WLB`](https://explorer.solana.com/address/EtXNoNoYQdF9QFaSttjGzZkAk29EcBW9apdcE4En6WLB?cluster=devnet) | trilltino/raydium-transfer-hook programs/arbitrary-test-hook @ 9cc41da |

Admin of both Raydium builds and upgrade authority of all four programs: [`QHgnAZswA5wt8ABUv5n7yM4FXFJdNwLsNYXKSVKB1Pm`](https://explorer.solana.com/address/QHgnAZswA5wt8ABUv5n7yM4FXFJdNwLsNYXKSVKB1Pm?cluster=devnet).
CPMM pool-creation fee receiver (a wrapped-SOL token account): [`CnoYEaFeS92rnvfY7i1WHY3yKnXgUQqs1xiC1eVj2aYS`](https://explorer.solana.com/address/CnoYEaFeS92rnvfY7i1WHY3yKnXgUQqs1xiC1eVj2aYS?cluster=devnet).
Machine-readable copy, with every transaction: [`environments/devnet.json`](../environments/devnet.json).

## Deployed artifacts

| Name | Program | Bytes | SHA-256 | Deploy transaction |
|---|---|---|---|---|
| reference-hook | [`Cz3Ge1ENd1yZAbtXA88dYaxA8x4ZkSxxdHmSj7nxQ11X`](https://explorer.solana.com/address/Cz3Ge1ENd1yZAbtXA88dYaxA8x4ZkSxxdHmSj7nxQ11X?cluster=devnet) | 141,424 | `11c7dd3410d8…` | [`2AV4xF4h…6GCNcZ`](https://explorer.solana.com/tx/2AV4xF4hUyn8L7yGDJBTh8nM4yEhvdQt8YMQc7kkFYV7cXbiMh2LckAYQsvA3KbLHnVhpKYLhMfdEw9ps66GCNcZ?cluster=devnet) |
| arbitrary-test-hook | [`EtXNoNoYQdF9QFaSttjGzZkAk29EcBW9apdcE4En6WLB`](https://explorer.solana.com/address/EtXNoNoYQdF9QFaSttjGzZkAk29EcBW9apdcE4En6WLB?cluster=devnet) | 125,928 | `5055f7371daa…` | [`bySBbdTR…etZP1Z`](https://explorer.solana.com/tx/bySBbdTRqEfdt47n8NrgMc6vge6FmsLuozr9hdcBH6efK3nQ9em7BUmDjHUsfQ5LC69NKefGmeLjfvAzMetZP1Z?cluster=devnet) |
| cpmm | [`7tRJH4mmEfNGGLf9E8qEvo3oSjq2b75DhSggb1Wz45fJ`](https://explorer.solana.com/address/7tRJH4mmEfNGGLf9E8qEvo3oSjq2b75DhSggb1Wz45fJ?cluster=devnet) | 596,720 | `52a0efaba62e…` | [`2e7f7e4U…pukURm`](https://explorer.solana.com/tx/2e7f7e4UwjtmVCSTpAfJzST3R5CmgonJTYVHPnB8JDh7moAvstR8tLkqYBKURp83MoTijXX1BQHnQw5mAppukURm?cluster=devnet) |
| clmm | [`3dNJapViueBArDN3HbWKcEf2u6hfZQ3oDyh3GmUXJ8oD`](https://explorer.solana.com/address/3dNJapViueBArDN3HbWKcEf2u6hfZQ3oDyh3GmUXJ8oD?cluster=devnet) | 1,152,864 | `3d4151fab3a1…` | [`hHxR4gjd…m7dK8k`](https://explorer.solana.com/tx/hHxR4gjdgNA6ynHXyQcGLtApfhG618XEB6c65Jt5CBpXPvnLSqDLUj4s4PWtTpamefAYGDzYdPJ6PQeyzm7dK8k?cluster=devnet) |

Rent is a refundable deposit held by each program-data account (measured with `solana rent`):
hook 0.72 SOL, arbitrary hook about 0.88, CPMM 3.03, CLMM 5.86. Closing a program returns it to the
upgrade authority.

## What ran on devnet

`raydium-hook e2e --env environments/devnet.json --amm all --hook all --record` ran four flows,
each of which performs real admin setup, creates a hooked Token-2022 mint and a plain quote mint, a
real pool (and for CLMM a liquidity position that creates the tick arrays), enables the hook, then:

1. a hooked swap with the hooked token as input, then as output (the hook must run exactly once,
   and a stateful hook's account must change);
2. a swap the hook refuses, in each direction: it must fail inside the hook program with the hook's
   own error code, and every balance must be unchanged afterwards.

| AMM | Hook | Hooked token in | Hooked token out | Refusal |
|---|---|---|---|---|
| CPMM | reference-hook | [`2A3MeK12…3Jthmf`](https://explorer.solana.com/tx/2A3MeK12sSwewRBxoZczBBAB15wGoVHHXqErFuZRPwXjW2AeMJ5xQzpgmTqSNyG1Wh3dawowt3rDfnh1Zq3Jthmf?cluster=devnet) (72,524 CU simulated) | [`2WAvXdb8…RfuK5R`](https://explorer.solana.com/tx/2WAvXdb8MkfLtYt6x8oQehoxqp3n4jpxqPuhSi5ch5MPDV95oPfseKdy8emWUogu6cY12HQj4Z5mpYKuCKRfuK5R?cluster=devnet) (72,851 CU simulated) | refused with `0x700b` in both directions |
| CPMM | arbitrary-test-hook | [`4aDR541H…LgKePv`](https://explorer.solana.com/tx/4aDR541HLMWc2axj77S9CF9bQBVXNH81YzC5qWWmz7S9fAoWPS6q852TxgA3wVyvXZPKJVNNctae2y2969LgKePv?cluster=devnet) (82,036 CU simulated) | [`v1GUCqZw…oKXA7Q`](https://explorer.solana.com/tx/v1GUCqZwsiunk4JGvqE3DadFtE5kW3Bqb4CJT7Gg7UniB8f73jzGthsQkcGVJq8B6fQh5Tad6LzpsgdLAoKXA7Q?cluster=devnet) (82,363 CU simulated) | refused with `0x9001` in both directions |
| CLMM | reference-hook | [`67UuoVHe…mhXFK8`](https://explorer.solana.com/tx/67UuoVHejRLJnSCNsYS4vRC6dX2bVn5TvPUDN1oSwTQyJm2QEiWcQxpWH2LqDQQ2sat2ntBGomBR1Le47SmhXFK8?cluster=devnet) (111,866 CU simulated) | [`5dScczBq…uC28Yg`](https://explorer.solana.com/tx/5dScczBqJ5MM6uqAAVbp7hphvLUY6pMQn2ouWz2WwxyGVBKQJWSqhJp7MFS62NALaAJejNzdgwv14z5mDsuC28Yg?cluster=devnet) (109,081 CU simulated) | refused with `0x700b` in both directions |
| CLMM | arbitrary-test-hook | [`aTKCgSW2…Qx5EFW`](https://explorer.solana.com/tx/aTKCgSW24fJXyWtPw8dYQMHuPwN6BwwnEVzschh3EkcRzmJjJ8kMmPeZUuhjD3CnMNLSAj52Xg2Pcxu2MQx5EFW?cluster=devnet) (113,814 CU simulated) | [`2v4oWm5H…sZTdEt`](https://explorer.solana.com/tx/2v4oWm5HJtGRqn5a3Bq8gYoViks3UFbwFMadXTuQQgSZTBSBQyCwpH8k4BpMjbprCMWSovDFjE4E5PaSnmsZTdEt?cluster=devnet) (111,029 CU simulated) | refused with `0x9001` in both directions |

All four flows reported PASS. The arbitrary hook's refusal sends three swaps in one transaction;
the hook allows two per slot and refuses the third, so the whole transaction, including the two
swaps that had already executed, rolls back.

Pools created in that run:

- CPMM / reference-hook: program 7tRJH4mmEfNGGLf9E8qEvo3oSjq2b75DhSggb1Wz45fJ pool 7Aax19pFDHGYfhDH4azpGFiKNSf6sPTm4QfXeqoDSd4y hooked mint 5Bf1yZPeCwDAhkwZTSppfsKX4MTuckwgmehyezQyiNh7 quote mint 5PoNS6Djy8MQExVFp3MFoLX4cMRJyW4eKvMdTDRqisXc
- CPMM / arbitrary-test-hook: program 7tRJH4mmEfNGGLf9E8qEvo3oSjq2b75DhSggb1Wz45fJ pool 5LPEEURCodbb1LK8HZkj13GWU1cQdo9zSNaSkNFmvTcH hooked mint 8sVD4FTZQBvq5NKR9zzbRuWjMAngLqwZEX4aGCTJx5aD quote mint GhUfDvBFcZs5dGaUP46RBi4zX4GhWdARHRjMrN2C39UB
- CLMM / reference-hook: program 3dNJapViueBArDN3HbWKcEf2u6hfZQ3oDyh3GmUXJ8oD pool 7KBH7PaDKm3GiXruzgWno3jYQPEA1mJ1hgnadaL1L4Dw hooked mint 8hvLoLnbowTnwg33QyxsNnBgGAPcfcSg8miQwBiJvVy quote mint 9yp26sbfaR2xgheHcKBkV4qskxHK1xmscBxBFPeA3Nwe
- CLMM / arbitrary-test-hook: program 3dNJapViueBArDN3HbWKcEf2u6hfZQ3oDyh3GmUXJ8oD pool 5QzkSCJSLN8v43sc4V1e1gE4MittajEhpnRNsDrytRTH hooked mint 58VnL5wUCw5BrNRoAoskazaK9ENc519GPJHLY4qCAzH quote mint rKLpAkGbABQ47fp5GkKkkacpjggrPMEyghQXbDHfjTM

To check one by hand: `solana confirm -v <signature> --url devnet` shows the call chain
Raydium program, then Token-2022 `TransferChecked`, then the hook, all succeeding.

## Reproduce

```powershell
cargo xtask upstream fetch --hook --locked      # the pinned fork commits -> target\upstream\{cpmm,clmm}-hook
cargo build-sbf --manifest-path target\upstream\cpmm-hook\programs\cp-swap\Cargo.toml `
  --sbf-out-dir target\integration-sbf -- --features integration
cargo build-sbf --manifest-path target\upstream\clmm-hook\programs\amm\Cargo.toml `
  --sbf-out-dir target\integration-sbf -- --features integration
cargo build-sbf --manifest-path programs\reference-hook-onchain\Cargo.toml --sbf-out-dir target\integration-sbf
cargo build-sbf --manifest-path programs\arbitrary-test-hook\Cargo.toml --sbf-out-dir target\integration-sbf

raydium-hook deploy --env environments\devnet.json --keypair .keys\deployer.json `
  --artifacts target\integration-sbf --keys .keys
raydium-hook e2e --env environments\devnet.json --keypair .keys\deployer.json `
  --fee-receiver-keypair .keys\cpmm-fee-receiver.json --amm all --hook all --record
```

The program and admin keypairs live in `.keys/` (git-ignored, never committed). Rebuilding from the
same commits gives the artifacts above only if the toolchain matches; compare against the SHA-256
column. A fresh deployment needs its own program ids and a matching `integration` build.

The same artifacts run in-process (no network) with
`cargo test -p raydium-hook-driver --features local --test local_flows -- --ignored`.

## Limits of this evidence

- A test environment under our own program ids. It says nothing about official Raydium, whose
  programs reject these instructions.
- Single runs on public devnet; no load, no timing, no transaction-size or address-lookup-table
  measurements.
- The deployer is the upgrade authority of every program here, so the code can be replaced at any
  time. That is a test-environment choice, not a recommendation.
- Liquidity deposits and withdrawals are not covered: those paths reject hooked mints.
