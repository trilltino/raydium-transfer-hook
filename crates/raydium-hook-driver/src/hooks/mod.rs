//! Hook setup providers. A Transfer Hook defines how it executes, not how it is initialised, so
//! setup is delegated to a provider. The flows and the Raydium builders only see [`HookSetup`]; a
//! new hook adds a provider and nothing else changes.

mod arbitrary;
mod creator_commitment;
mod fair_launch;
mod generic;
mod holder_rewards;
mod reference;

use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair};
use spl_token_2022::extension::transfer_hook::instruction as transfer_hook_instruction;
use transfer_hook_sdk::SplTransferLeg;

pub use arbitrary::ArbitraryHook;
pub use creator_commitment::CreatorCommitmentHook;
pub use fair_launch::FairLaunchHook;
pub use generic::{GenericExternalHook, GenericHookSpec};
pub use holder_rewards::HolderRewardsHook;
pub use reference::ReferenceHook;

/// Which side of a swap the hooked token is on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// The trader sends the hooked token into the pool (a sell).
    HookedIn,
    /// The trader receives the hooked token from the pool (a buy).
    HookedOut,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Direction::HookedIn => "hooked token in",
            Direction::HookedOut => "hooked token out",
        }
    }
}

/// How a flow can make a hooked swap fail, deterministically, on any cluster.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectionPlan {
    /// A swap whose input amount trips an amount rule of the hook.
    OverAmount { amount_in: u64 },
    /// `times` swaps inside one transaction (more than the hook allows per slot per mint).
    RepeatInOneTransaction { times: usize },
    /// A swap sent with a compute-unit price (micro-lamports per unit) the hook refuses.
    HighPriorityFee { amount_in: u64, micro_lamports: u64 },
}

impl RejectionPlan {
    pub fn label(self) -> &'static str {
        match self {
            RejectionPlan::OverAmount { .. } => "over-amount",
            RejectionPlan::RepeatInOneTransaction { .. } => "repeated in one transaction",
            RejectionPlan::HighPriorityFee { .. } => "high priority fee",
        }
    }
}

/// One swap the hook must refuse: which direction, how to provoke it, and the error code the
/// hook must fail with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Refusal {
    pub direction: Direction,
    pub plan: RejectionPlan,
    pub code: u32,
}

/// What a provider needs to know about the pool a flow just created.
#[derive(Clone, Debug)]
pub struct HookContext {
    /// The wallet that runs setup, owns the trader accounts and is the mint's hook authority.
    pub payer: Pubkey,
    pub hooked_mint: Pubkey,
    pub quote_mint: Pubkey,
    /// The trader's token accounts, `[hooked, quote]`, owned by `payer`.
    pub trader_accounts: [Pubkey; 2],
    /// The owner of the pool vaults: CPMM's authority PDA, CLMM's pool-state PDA.
    pub pool_authority: Pubkey,
    /// The pool vaults, `[hooked, quote]`.
    pub vaults: [Pubkey; 2],
    /// Chain unix time when setup ran.
    pub now: i64,
}

/// A step a hook needs after the standard swap checks (fund a vault, claim, snapshot...).
pub enum FollowUp {
    /// Send a transaction; `signers` sign in addition to the payer.
    Send {
        label: String,
        instructions: Vec<Instruction>,
        signers: Vec<Keypair>,
    },
    /// Send a transaction that the hook must refuse with `code`; nothing may change.
    SendExpectFailure {
        label: String,
        instructions: Vec<Instruction>,
        signers: Vec<Keypair>,
        code: u32,
    },
    /// Let at least this many seconds of cluster time pass.
    AdvanceTime(u64),
    /// Remember a token account's balance under `name`, for [`FollowUp::ExpectChange`].
    Remember { name: String, account: Pubkey },
    /// Require a token account's balance to differ from the one remembered under `since` by an
    /// amount in `min..=max`.
    ExpectChange {
        label: String,
        account: Pubkey,
        since: String,
        min: i64,
        max: i64,
    },
    /// A swap of `amount_in` in one direction that must succeed.
    Swap {
        label: String,
        direction: Direction,
        amount_in: u64,
    },
    /// Require a token account's balance to lie in `min..=max`.
    ExpectTokenBalance {
        label: String,
        account: Pubkey,
        min: u64,
        max: u64,
    },
}

pub trait HookSetup {
    fn name(&self) -> &'static str;
    fn program_id(&self) -> Pubkey;

    /// Run by the mint's TransferHook authority: point the mint at the hook, then initialise the
    /// hook's per-mint state for this pool.
    fn enable_instructions(&self, ctx: &HookContext) -> Vec<Instruction>;

    /// Every swap the hook must refuse. A rule that binds only one direction lists only that
    /// direction.
    fn refusals(&self) -> Vec<Refusal>;

    /// An account the hook writes on every allowed transfer, if it keeps state: the flow checks
    /// that it changed after a hooked swap.
    fn state_account(&self, _mint: &Pubkey) -> Option<Pubkey> {
        None
    }

    /// The writable extra accounts the integrator accepts for this leg. By default only the state
    /// account. The SDK refuses any other writable extra.
    fn allowed_writable(&self, ctx: &HookContext, _leg: &SplTransferLeg) -> Vec<Pubkey> {
        self.state_account(&ctx.hooked_mint).into_iter().collect()
    }

    /// Steps to run after the standard swap checks.
    fn follow_up(&self, _ctx: &HookContext) -> Vec<FollowUp> {
        Vec::new()
    }
}

/// The instruction that points `mint` at `program` (run by the extension authority).
pub(crate) fn point_mint_at_hook(
    mint: &Pubkey,
    authority: &Pubkey,
    program: &Pubkey,
) -> Instruction {
    transfer_hook_instruction::update(&spl_token_2022::id(), mint, authority, &[], Some(*program))
        .expect("TransferHook update")
}
