#![forbid(unsafe_code)]

use hook_policy_model::TransferContext;
use transfer_hook_sdk::{
    ResolveError, ResolvedAccountBatch, TransferHookAccountSource, TransferHookResolver,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpmmPath {
    SwapBaseInput,
    Deposit,
    Withdraw,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CpmmTransferPlan {
    pub path: CpmmPath,
    pub transfers: ResolvedAccountBatch,
}

pub fn resolve_swap_base_input<S: TransferHookAccountSource>(
    resolver: &TransferHookResolver,
    source: &mut S,
    input_transfer: TransferContext,
    output_transfer: TransferContext,
) -> Result<CpmmTransferPlan, ResolveError> {
    resolve_two_transfer_path(
        CpmmPath::SwapBaseInput,
        resolver,
        source,
        input_transfer,
        output_transfer,
    )
}

pub fn resolve_deposit<S: TransferHookAccountSource>(
    resolver: &TransferHookResolver,
    source: &mut S,
    token_0_transfer: TransferContext,
    token_1_transfer: TransferContext,
) -> Result<CpmmTransferPlan, ResolveError> {
    resolve_two_transfer_path(
        CpmmPath::Deposit,
        resolver,
        source,
        token_0_transfer,
        token_1_transfer,
    )
}

pub fn resolve_withdraw<S: TransferHookAccountSource>(
    resolver: &TransferHookResolver,
    source: &mut S,
    token_0_transfer: TransferContext,
    token_1_transfer: TransferContext,
) -> Result<CpmmTransferPlan, ResolveError> {
    resolve_two_transfer_path(
        CpmmPath::Withdraw,
        resolver,
        source,
        token_0_transfer,
        token_1_transfer,
    )
}

fn resolve_two_transfer_path<S: TransferHookAccountSource>(
    path: CpmmPath,
    resolver: &TransferHookResolver,
    source: &mut S,
    first: TransferContext,
    second: TransferContext,
) -> Result<CpmmTransferPlan, ResolveError> {
    Ok(CpmmTransferPlan {
        path,
        transfers: resolver.resolve_batch(source, &[first, second])?,
    })
}

pub fn remaining_account_count(plan: &CpmmTransferPlan) -> usize {
    plan.transfers.accounts.len()
}
