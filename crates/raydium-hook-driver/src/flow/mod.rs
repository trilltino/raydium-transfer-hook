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
mod liquidity;
mod recorder;
mod support;
mod swaps;
mod world;

use crate::{env::Environment, hooks::HookSetup};
use solana_sdk::signature::Keypair;

pub use clmm::{run_clmm, run_clmm_session};
pub use cpmm::{run_cpmm, run_cpmm_session};
pub use support::resolve as resolve_swap_leg;

/// Swap size that every hook in this repository allows.
pub const SWAP_AMOUNT: u64 = 10;

/// Leave a funded wallet and a live pool behind instead of running the swap checks: what a browser
/// test needs. The wallet gets associated token accounts for both mints, funded by the payer (who is
/// the mint authority), and some SOL for fees.
#[derive(Clone, Copy, Debug)]
pub struct UiFixture {
    pub wallet: solana_sdk::pubkey::Pubkey,
    /// Raw units seeded into the pool for each token.
    pub seed_amount: u64,
    /// Raw units of each token minted to the wallet.
    pub wallet_amount: u64,
    pub wallet_lamports: u64,
}

pub struct FlowInputs<'a> {
    pub env: &'a Environment,
    /// The hook on the first mint (`mint_0`, the smaller pubkey).
    pub hook: &'a dyn HookSetup,
    /// The keypair at the CPMM fee-receiver address, needed only if that token account does not
    /// exist yet.
    pub fee_receiver_keypair: Option<&'a Keypair>,
    /// A hook on the other mint too, so both legs of every swap are hooked. Its hook sees the
    /// other mint as "its" mint (its context has the trader accounts and vaults swapped). Only
    /// hooks without follow-up steps are supported here.
    pub second_hook: Option<&'a dyn HookSetup>,
    /// Put a TransferFee extension of this many basis points on both mints (0 for none).
    pub transfer_fee_bps: u16,
    /// After the standard checks, also swap for an exact output amount (CPMM `swap_base_output`,
    /// framed as `swap_base_output_v2`). Only AMMs that have such an instruction support this.
    pub exact_output: bool,
    /// After the standard checks, also run the CPMM operations that move two tokens with the
    /// hook live: pool creation, deposit, withdraw and fee collection (`*_v2`). CPMM only.
    pub liquidity: bool,
    /// Set up a pool and a funded wallet for a browser test, and stop before the swap checks. CPMM only.
    pub ui_fixture: Option<UiFixture>,
}

impl<'a> FlowInputs<'a> {
    /// One hook on `mint_0`, plain mints otherwise.
    pub fn new(
        env: &'a Environment,
        hook: &'a dyn HookSetup,
        fee_receiver_keypair: Option<&'a Keypair>,
    ) -> Self {
        Self {
            env,
            hook,
            fee_receiver_keypair,
            second_hook: None,
            transfer_fee_bps: 0,
            exact_output: false,
            liquidity: false,
            ui_fixture: None,
        }
    }

    pub fn with_ui_fixture(mut self, fixture: UiFixture) -> Self {
        self.ui_fixture = Some(fixture);
        self
    }

    pub fn with_second_hook(mut self, hook: &'a dyn HookSetup) -> Self {
        self.second_hook = Some(hook);
        self
    }

    pub fn with_transfer_fee(mut self, basis_points: u16) -> Self {
        self.transfer_fee_bps = basis_points;
        self
    }

    pub fn with_liquidity(mut self) -> Self {
        self.liquidity = true;
        self
    }

    pub fn with_exact_output(mut self) -> Self {
        self.exact_output = true;
        self
    }

    fn world_options(&self) -> world::WorldOptions {
        world::WorldOptions {
            quote_hooked: self.second_hook.is_some(),
            transfer_fee_bps: self.transfer_fee_bps,
        }
    }
}
