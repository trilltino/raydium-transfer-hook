# legacy/: waiting for the official Raydium interface

**Not part of the kit. Do not build on it, copy from it, or document it to hook authors.**

This is code from the prototype that ran arbitrary Transfer Hooks through modified Raydium CPMM and
CLMM programs. It is kept, for now, only because deleting it before Raydium publishes its own
interface could lose something still useful during the transition. Classification:
**WAIT FOR OFFICIAL RAYDIUM ABI**. Delete it when that interface exists.

| Directory | What it was | When it goes |
|---|---|---|
| `raydium-adapters/` | Instruction builders for the prototype's hook-aware CPMM `*_v2` and CLMM `*_v3` instructions (fixed account layouts, discriminators) | Delete once Raydium's SDK handles hooks (option A), or shrink to a minimal helper only if a real gap remains (option B). Do not grow it into a competing Raydium SDK |
| `transfer-hook-sdk/` | Resolves a transfer leg's hook accounts and frames them into those instructions; privilege policy; golden ABI fixtures | Same. The generic part (resolve extras) is replaced by `spl_token_2022::offchain` / `@solana/spl-token` |
| `hook-policy-model/` | An off-chain model of how a platform could select a hook per platform (LaunchLab idea). Never deployed | Archive or delete |
| `transfer-hook-client/` | The TypeScript equivalent (resolve, frame, quote, simulate). Its generic helpers (inspect a mint, decode hook errors) are the only potentially reusable part | Delete with the Raydium builders; prefer Raydium SDK V2 if it handles hooks |

It is a separate Cargo workspace (`cargo test --manifest-path legacy/Cargo.toml`) and an npm package
(`cd legacy/transfer-hook-client && npm install && npx vitest run`). Both passed when it was moved
here. The fork program ids, deployment manifests, devnet evidence and CLI were deleted; they remain
in git history (tag `pre-community-hook-kit`).
