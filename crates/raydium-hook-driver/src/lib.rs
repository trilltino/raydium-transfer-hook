//! Runs the Raydium + Transfer Hook end-to-end flows against any chain.
//!
//! * [`chain`]: the [`chain::Chain`] abstraction, a real RPC endpoint or `solana-program-test`.
//! * [`env`]: environment manifests; program ids always come from one.
//! * [`hooks`]: hook setup providers, so a new hook adds a provider and nothing else (or, for a
//!   hook known only by program id, a JSON description: [`hooks::GenericExternalHook`]).
//! * [`readiness`]: the transport facts of a hooked mint, kept apart from business readiness.
//! * [`cpmm`], [`clmm`]: instruction builders for the Raydium programs.
//! * [`flow`]: the checked end-to-end flows.

pub mod chain;
pub mod clmm;
pub mod cpmm;
pub mod env;
pub mod flow;
pub mod hooks;
pub mod readiness;
pub mod token;

#[cfg(feature = "local")]
pub use chain::LocalChain;
pub use chain::{Chain, DriverError, RpcChain};
pub use env::{Environment, Evidence};
pub use flow::{run_clmm, run_cpmm, FlowInputs};
pub use hooks::{
    AntiBundleHook, ArbitraryHook, CreatorCommitmentHook, Direction, FairLaunchHook, FollowUp,
    GenericExternalHook, HookContext, HookSetup, LoyaltyRewardsHook, ParentSpinOffHook,
    ReferenceHook, Refusal, RejectionPlan,
};
pub use readiness::{inspect_readiness, Readiness};
