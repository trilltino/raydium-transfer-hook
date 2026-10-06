# Architecture

## Scope

The workspace is a local reference harness for the account and policy flow around a Token-2022 Transfer Hook. It does not include the Raydium or Token-2022 on-chain programs and does not execute real CPIs.

## Implemented boundaries

1. **Client resolver (`transfer-hook-sdk`):** A provider supplies a freshly fetched typed mint, the derived validation-list address, typed validation-list state, and SPL-compatible resolution of that list for a transfer's exact source, mint, destination, and authority. The resolver validates owner, key, mint, minimum layout length, and the Execute-list marker on every call.
2. **Transfer-specific plan:** Every transfer has a distinct contiguous account range. Lists are appended in transfer order without global deduplication or privilege merging. The ordered helper output models SPL's off-chain helper: resolved additional metas, hook program, then validation list.
3. **Reference engine:** `reference-hook` validates per-mint module allow-lists and mutually exclusive address-policy modules, evaluates transfer limits/address rules, and models platform-retained, immutable, or timelocked configuration authority.
4. **Product-flow models:** CPMM swap/deposit/withdraw, CLMM SwapV2 remaining-account partitioning, and LaunchLab mint/trade/graduation lifecycle are represented for local testing only.

## Upstream ABI result

- **CPMM:** The reviewed transfer helper invokes `TransferChecked` with only source, destination, authority, and mint. The swap/deposit/withdraw handlers do not forward `Context::remaining_accounts` to those helpers. Existing non-hook V1 behavior can remain unchanged, but a live hook path needs explicit per-transfer account plumbing and must not guess a union.
- **CLMM:** `SwapV2` iterates remaining accounts by data length as tick arrays or a bitmap extension and stops at the first other account. Its transfer helpers receive fixed accounts only. A hook tail is not safely described by this parser; the local model keeps tick/bitmap and transfer slices separate, but does not extend the live ABI.
- **LaunchLab:** The public SDK provides account layouts and discriminators, including a Token-2022 mint-creation builder, but no public on-chain Rust handler was available in the source review. Initialization, buy/sell transfer count, and graduation persistence therefore remain unverified.

Full source references and frozen discriminator bytes are in the [transfer-surface matrix](transfer-surface-matrix.md).

## Account union

`TransferAccountPlan::union` remains a utility for policy-model tests only. Product adapters do not use it. Per-transfer ranges intentionally preserve duplicate account keys and privileges, because the reviewed handlers do not define a safe mapping from one deduplicated union to multiple Token-2022 CPIs.

## Atomicity

The E2E test stages local balance changes and commits only after all modeled hook checks pass. This tests the harness's all-or-nothing behavior, not Solana runtime rollback. Runtime atomicity and actual hook rejection still require validator-based integration tests against a concrete Raydium fork.
