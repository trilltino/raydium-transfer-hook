//! Seeds, discriminators and account sizes.

pub const POLICY_SEED: &[u8] = b"arb-policy";
pub const STATS_SEED: &[u8] = b"arb-stats";
pub const VALIDATION_LIST_SEED: &[u8] = b"extra-account-metas";
pub const POLICY_DISCRIMINATOR: [u8; 8] = *b"ARBPOLCY";
pub const STATS_DISCRIMINATOR: [u8; 8] = *b"ARBSTATS";
pub const INIT_DISCRIMINATOR: [u8; 8] = *b"ARBINIT1";
pub const EXECUTE_DISCRIMINATOR: [u8; 8] = [105, 37, 101, 197, 75, 251, 102, 26];
pub const POLICY_LEN: usize = 8 + 1 + 32 + 4;
pub const STATS_LEN: usize = 8 + 1 + 8 + 4 + 8;
/// Resolved extra accounts beyond the four base keys and the validation list.
pub const EXTRA_ACCOUNTS: usize = 2;
pub(crate) const EXECUTE_ACCOUNT_COUNT: usize = 5 + EXTRA_ACCOUNTS;
