//! Instruction builders for the external Raydium programs. Raydium is **not** part of this
//! repository: these build instructions *for* its CPMM and CLMM programs, parameterised by the
//! program id from an environment manifest (nothing here is tied to Raydium's mainnet or devnet
//! ids), and assemble hooked swaps by handing each transfer leg's accounts to the SDK's framers.
//!
//! | Module | Role |
//! |---|---|
//! | [`cpmm`] | CP-Swap: admin setup, pool creation, the swap's fixed accounts |
//! | [`clmm`] | CLMM: admin setup, pool and position creation, the swap's fixed accounts |
//! | [`swap`] | a hooked swap instruction from resolved legs (`swap_base_input_v2`, `swap_v3`) |
//! | [`token`] | Token-2022 mints with TransferHook / TransferFee extensions, and token accounts |
//!
//! The admin-only instructions (AmmConfig, support mints) exist only in the hook-support forks'
//! `integration` builds; see `docs/upstream-sources.md`.

pub mod clmm;
pub mod cpmm;
pub mod swap;
pub mod token;
