//! Small helpers shared by the flows: checks, transaction sending, account reads, hook setup.

use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use transfer_hook_sdk::{
    resolve_leg, FetchError, LegHook, LegRole, PrivilegePolicy, ResolveOptions, SplAccount,
    SplTransferLeg,
};

use super::recorder::Recorder;
use crate::{
    chain::{Chain, DriverError, Result},
    hooks::HookSetup,
    token,
};

const COMPUTE_UNITS: u32 = 1_400_000;

pub(super) fn require(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(DriverError::new(message))
    }
}

pub(super) fn with_budget(mut instructions: Vec<Instruction>) -> Vec<Instruction> {
    instructions.insert(
        0,
        ComputeBudgetInstruction::set_compute_unit_limit(COMPUTE_UNITS),
    );
    instructions
}

pub(super) async fn send_step<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    step: &str,
    instructions: Vec<Instruction>,
    signers: &[&Keypair],
) -> Result<()> {
    let sent = chain
        .send(&instructions, signers)
        .await
        .map_err(|e| DriverError::new(format!("step `{step}` failed: {e}")))?;
    rec.push(step, Some(sent.signature), "");
    Ok(())
}

pub(super) async fn amount_of<C: Chain>(chain: &mut C, account: &Pubkey) -> Result<u64> {
    let data = chain
        .account(account)
        .await?
        .ok_or_else(|| DriverError::new(format!("token account {account} does not exist")))?
        .data;
    token::token_amount(&data)
        .ok_or_else(|| DriverError::new(format!("{account} is not a Token-2022 account")))
}

pub(super) async fn raw_data<C: Chain>(chain: &mut C, account: &Pubkey) -> Result<Vec<u8>> {
    Ok(chain
        .account(account)
        .await?
        .ok_or_else(|| DriverError::new(format!("account {account} does not exist")))?
        .data)
}

pub(super) async fn resolve<C: Chain>(
    chain: &C,
    role: LegRole,
    leg: SplTransferLeg,
    expected_hook: Option<Pubkey>,
    writable_extras: Vec<Pubkey>,
) -> Result<LegHook> {
    let reader = chain.reader();
    let mut options = ResolveOptions::default();
    if let Some(program) = expected_hook {
        options = options.with_expected_hook_program(program);
    }
    // The only writable extra the integrator accepts is the hook's own state account.
    if !writable_extras.is_empty() {
        options =
            options.with_privilege_policy(PrivilegePolicy::allowing_writable(writable_extras));
    }
    resolve_leg(role, leg, &options, move |key| {
        let reader = reader.clone();
        async move {
            match reader(key).await {
                Ok(Some(account)) => Ok(Some(SplAccount {
                    key,
                    owner: account.owner,
                    data: account.data,
                    executable: account.executable,
                })),
                Ok(None) => Ok(None),
                Err(e) => Err(FetchError::new(e.message)),
            }
        }
    })
    .await
    .map_err(|e| DriverError::new(format!("hook resolution failed: {e:?}")))
}

pub(super) async fn enable_hook<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    hook: &dyn HookSetup,
    mint: &Pubkey,
) -> Result<()> {
    let payer = chain.payer().pubkey();
    let sent = chain
        .send(&hook.enable_instructions(mint, &payer, &payer), &[])
        .await
        .map_err(|e| DriverError::new(format!("enabling the hook failed: {e}")))?;
    rec.push(
        &format!("enable {} on the hooked mint", hook.name()),
        Some(sent.signature),
        format!("hook program {}", hook.program_id()),
    );
    Ok(())
}
