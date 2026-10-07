//! The end-to-end flows. Each step is checked; a check that does not hold returns an error, so a
//! flow can only finish by actually proving what it claims:
//!
//! 1. admin setup (AmmConfig, fee receiver, support mint for the hooked mint) with real admin
//!    instructions signed by the integration build's admin;
//! 2. a hooked Token-2022 mint and a plain Token-2022 mint, funded accounts;
//! 3. a real pool (CPMM `initialize`, or CLMM `create_pool` + a liquidity position);
//! 4. enable the hook on the hooked mint (the liquidity paths reject hooked mints, so the hook
//!    goes on after the pool has liquidity);
//! 5. hooked swap with the hooked token as input, then as output: each simulated first (the hook
//!    must run exactly once and the hook's state must change if it keeps state), then sent;
//! 6. a swap the hook refuses: it must fail inside the hook program with the hook's error code,
//!    and every balance must be unchanged afterwards.
//!
//! The hooked mint is always `mint_0`, so the first swap is `zero_for_one` on CLMM.

mod clmm;
mod cpmm;
mod recorder;
mod support;
mod swaps;
mod world;

use crate::{env::Environment, hooks::HookSetup};
use solana_sdk::signature::Keypair;

pub use clmm::run_clmm;
pub use cpmm::run_cpmm;

/// Swap size that every hook in this repository allows.
pub const SWAP_AMOUNT: u64 = 10;

pub struct FlowInputs<'a> {
    pub env: &'a Environment,
    pub hook: &'a dyn HookSetup,
    /// The keypair at the CPMM fee-receiver address, needed only if that token account does not
    /// exist yet.
    pub fee_receiver_keypair: Option<&'a Keypair>,
}
