//! Runs the Raydium + Transfer Hook end-to-end flows against any chain.
//!
//! * [`chain`]: the [`chain::Chain`] abstraction, a real RPC endpoint or `solana-program-test`.
//! * [`env`]: environment manifests; program ids always come from one.
//! * [`hooks`]: hook setup providers, so a new hook adds a provider and nothing else (or, for a
//!   hook known only by program id, a JSON description: [`hooks::GenericExternalHook`]).
//! * [`readiness`]: the transport facts of a hooked mint, kept apart from business readiness.
//! * [`cpmm`], [`clmm`], [`swap`], [`token`]: instruction builders for the Raydium programs, from
//!   the `raydium-adapters` crate.
//! * [`flow`]: the checked end-to-end flows.

pub mod chain;
pub mod env;
pub mod flow;
pub mod hooks;
pub mod readiness;
pub mod report;
pub mod session;

/// The instruction builders for the external Raydium programs live in `raydium-adapters`.
pub use raydium_adapters::{clmm, cpmm, swap, token};

#[cfg(feature = "local")]
pub use chain::LocalChain;
pub use chain::{Chain, DriverError, RpcChain};
pub use env::{Environment, Evidence};
pub use flow::{
    resolve_swap_leg, run_clmm, run_clmm_session, run_cpmm, run_cpmm_session, FlowInputs,
};
pub use hooks::{
    AntiBundleHook, ArbitraryHook, CreatorCommitmentHook, Direction, FairLaunchHook, FollowUp,
    GenericExternalHook, HookContext, HookSetup, LoyaltyRewardsHook, ParentSpinOffHook,
    ReferenceHook, Refusal, RejectionPlan,
};
pub use readiness::{inspect_program, inspect_readiness, ProgramFacts, Readiness, UpgradeInfo};
pub use report::{report, SimulationReport};
pub use session::Session;
