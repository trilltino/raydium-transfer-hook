//! The lifecycle phases a simulated launch moves through.

use hook_policy_model::Pubkey;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchPhase {
    MintCreated,
    HookInitialized,
    Trading,
    Graduated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraduationRecord {
    pub mint: Pubkey,
    pub hook_program: Option<Pubkey>,
    pub validation_list_initialized: bool,
}
