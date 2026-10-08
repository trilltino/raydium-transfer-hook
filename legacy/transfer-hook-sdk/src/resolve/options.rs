//! Knobs for resolution, and the program ids resolution refuses outright.

use solana_program::{
    bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable, pubkey, pubkey::Pubkey,
};

use super::privilege::PrivilegePolicy;
use crate::error::AuthorityExpectation;

/// LoaderV4. Spelled as a constant to avoid the deprecated `solana_program::loader_v4` re-export.
pub const LOADER_V4_ID: Pubkey = pubkey!("LoaderV411111111111111111111111111111111111");

/// Raydium program ids that must never be accepted as a transfer-hook program.
/// These are public on-chain addresses (CPMM and CLMM, mainnet and devnet,
/// plus AMM v4); no Raydium source is vendored here.
pub const RAYDIUM_PROGRAM_IDS: [Pubkey; 5] = [
    pubkey!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C"),
    pubkey!("DRaycpLY18LhpbydsBWbVJtxpNv9oXPgjRSfpF2bWpYb"),
    pubkey!("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK"),
    pubkey!("DRayAUgENGQBKVaX8owNhgzkEDyoHTGVEGHVJT1E9pfH"),
    pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"),
];

/// Knobs for [`resolve_leg`]. `Default` is strict on privileges and loaders but
/// does not pin a hook program: set `expected_hook_program` whenever the
/// caller knows which hook the mint should use.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ResolveOptions {
    /// If set, a mint whose hook is a different program (or has none) is rejected.
    pub expected_hook_program: Option<Pubkey>,
    /// Required state of the Transfer Hook extension authority.
    pub expected_hook_authority: AuthorityExpectation,
    pub privilege_policy: PrivilegePolicy,
    /// Loaders the hook program account may be owned by.
    pub allowed_loaders: Vec<Pubkey>,
    /// Reject mints that carry no hook.
    pub require_hook: bool,
    /// Re-read the mint, program, and list after resolving and fail if they
    /// changed. Narrows (but cannot close) the window in which the fetches
    /// observe different chain states; use [`HookFingerprint::verify_unchanged`]
    /// immediately before signing for the rest.
    pub recheck_consistency: bool,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            expected_hook_program: None,
            expected_hook_authority: AuthorityExpectation::Any,
            privilege_policy: PrivilegePolicy::reject_all(),
            allowed_loaders: default_allowed_loaders(),
            require_hook: false,
            recheck_consistency: true,
        }
    }
}

impl ResolveOptions {
    pub fn with_expected_hook_program(mut self, program: Pubkey) -> Self {
        self.expected_hook_program = Some(program);
        self
    }

    pub fn with_expected_hook_authority(mut self, expectation: AuthorityExpectation) -> Self {
        self.expected_hook_authority = expectation;
        self
    }

    pub fn with_privilege_policy(mut self, policy: PrivilegePolicy) -> Self {
        self.privilege_policy = policy;
        self
    }

    pub fn with_allowed_loaders(mut self, loaders: Vec<Pubkey>) -> Self {
        self.allowed_loaders = loaders;
        self
    }

    pub fn requiring_hook(mut self) -> Self {
        self.require_hook = true;
        self
    }

    /// Map a platform policy decision onto resolution options.
    ///
    /// `ImmutableAtLaunch` requires the Token-2022 hook authority to be revoked,
    /// otherwise the hook program could be swapped after launch.
    pub fn from_policy_decision(decision: &hook_policy_model::PolicyDecision) -> Self {
        let revoke_authority =
            decision.authority_policy.is_immutable_at_launch() && decision.hook_program.is_some();
        Self {
            expected_hook_program: decision.hook_program.map(Pubkey::new_from_array),
            expected_hook_authority: if revoke_authority {
                AuthorityExpectation::Revoked
            } else {
                AuthorityExpectation::Any
            },
            require_hook: decision.hook_required,
            ..Self::default()
        }
    }
}

/// Loaders accepted by default: BPF loader v2, deprecated v1, upgradeable, and v4.
pub fn default_allowed_loaders() -> Vec<Pubkey> {
    vec![
        bpf_loader::id(),
        bpf_loader_deprecated::id(),
        bpf_loader_upgradeable::id(),
        LOADER_V4_ID,
    ]
}
