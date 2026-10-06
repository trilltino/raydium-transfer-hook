#![forbid(unsafe_code)]

use hook_policy_model::{AccountMeta, TransferContext};
use transfer_hook_sdk::{
    ResolveError, ResolvedAccountBatch, TransferHookAccountSource, TransferHookResolver,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClmmSwapV2RemainingAccounts {
    pub tick_and_bitmap_accounts: Vec<AccountMeta>,
    pub transfer_accounts: ResolvedAccountBatch,
}

impl ClmmSwapV2RemainingAccounts {
    pub fn ordered_accounts(&self) -> Vec<AccountMeta> {
        self.tick_and_bitmap_accounts
            .iter()
            .chain(&self.transfer_accounts.accounts)
            .copied()
            .collect()
    }

    pub fn hook_account_range(&self) -> std::ops::Range<usize> {
        self.tick_and_bitmap_accounts.len()
            ..self.tick_and_bitmap_accounts.len() + self.transfer_accounts.accounts.len()
    }

    pub fn transfer_account_range(&self, transfer_index: usize) -> Option<std::ops::Range<usize>> {
        let transfer = self.transfer_accounts.transfers.get(transfer_index)?;
        let offset = self.tick_and_bitmap_accounts.len();
        Some(offset + transfer.accounts.start..offset + transfer.accounts.end)
    }
}

pub fn resolve_swap_v2_remaining_accounts<S: TransferHookAccountSource>(
    resolver: &TransferHookResolver,
    source: &mut S,
    tick_and_bitmap_accounts: Vec<AccountMeta>,
    input_transfer: TransferContext,
    output_transfer: TransferContext,
) -> Result<ClmmSwapV2RemainingAccounts, ResolveError> {
    let transfer_accounts = resolver.resolve_batch(source, &[input_transfer, output_transfer])?;
    Ok(ClmmSwapV2RemainingAccounts {
        tick_and_bitmap_accounts,
        transfer_accounts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_accounts_and_hook_tail_have_explicit_separate_ranges() {
        let plan = ClmmSwapV2RemainingAccounts {
            tick_and_bitmap_accounts: vec![
                AccountMeta::new([1; 32], false, true),
                AccountMeta::new([2; 32], false, true),
            ],
            transfer_accounts: ResolvedAccountBatch {
                accounts: vec![AccountMeta::new([3; 32], false, false)],
                transfers: Vec::new(),
            },
        };
        assert_eq!(plan.hook_account_range(), 2..3);
        assert_eq!(plan.transfer_account_range(0), None);
        assert_eq!(
            plan.ordered_accounts()
                .iter()
                .map(|account| account.key)
                .collect::<Vec<_>>(),
            vec![[1; 32], [2; 32], [3; 32]]
        );
    }
}
