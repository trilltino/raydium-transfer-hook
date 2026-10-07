//! Framing the CPMM operations that move two tokens — liquidity, fee collection and pool creation —
//! into their hook-aware `_v2` instructions.
//!
//! Each of them transfers token_0 and then token_1, so the framing is the same as a swap's with
//! two legs: `<name>_v2` carries two `u16` counts after the original arguments, and the remaining
//! accounts are the token_0 slice followed by the token_1 slice. The only differences between the
//! operations are the discriminators, the argument size and where each leg's accounts sit, which
//! is what [`CpmmPairOp`] tabulates.
//!
//! "Input" in a [`FramedSwap`] is token_0 here, and "output" is token_1.

use solana_program::instruction::{AccountMeta, Instruction};

use crate::{abi::anchor_instruction_discriminator, error::FrameError, resolve::LegHook};

use super::{
    layout::{validate_legs, LegLayout},
    FramedAbi, FramedSwap,
};

/// A CPMM operation that transfers token_0 and token_1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpmmPairOp {
    Deposit,
    Withdraw,
    CollectProtocolFee,
    CollectFundFee,
    CollectCreatorFee,
    CollectCreatorFeePermissionless,
    Initialize,
    InitializeWithPermission,
}

impl CpmmPairOp {
    pub const ALL: [CpmmPairOp; 8] = [
        Self::Deposit,
        Self::Withdraw,
        Self::CollectProtocolFee,
        Self::CollectFundFee,
        Self::CollectCreatorFee,
        Self::CollectCreatorFeePermissionless,
        Self::Initialize,
        Self::InitializeWithPermission,
    ];

    /// The Anchor instruction name of the original (V1) instruction.
    pub fn name(self) -> &'static str {
        match self {
            Self::Deposit => "deposit",
            Self::Withdraw => "withdraw",
            Self::CollectProtocolFee => "collect_protocol_fee",
            Self::CollectFundFee => "collect_fund_fee",
            Self::CollectCreatorFee => "collect_creator_fee",
            Self::CollectCreatorFeePermissionless => "collect_creator_fee_permissionless",
            Self::Initialize => "initialize",
            Self::InitializeWithPermission => "initialize_with_permission",
        }
    }

    pub fn v1_discriminator(self) -> [u8; 8] {
        anchor_instruction_discriminator(self.name())
    }

    pub fn v2_discriminator(self) -> [u8; 8] {
        anchor_instruction_discriminator(&format!("{}_v2", self.name()))
    }

    /// Length of the V1 instruction data: the discriminator and the arguments.
    pub fn v1_data_len(self) -> usize {
        8 + match self {
            Self::Deposit | Self::Withdraw | Self::Initialize => 24,
            Self::CollectProtocolFee | Self::CollectFundFee => 16,
            Self::CollectCreatorFee | Self::CollectCreatorFeePermissionless => 0,
            // three u64 and the one-byte `CreatorFeeOn`.
            Self::InitializeWithPermission => 25,
        }
    }

    /// The accounts in the V1 instruction's fixed list. Pool creation may carry support-mint
    /// records after them; the hook slices go in between.
    pub fn fixed_accounts(self) -> usize {
        match self {
            Self::Deposit => 13,
            Self::Withdraw => 14,
            Self::CollectProtocolFee | Self::CollectFundFee => 12,
            Self::CollectCreatorFee => 15,
            Self::CollectCreatorFeePermissionless => 16,
            Self::Initialize => 20,
            Self::InitializeWithPermission => 21,
        }
    }

    /// Whether the V1 instruction may carry more accounts after the fixed list (support-mint
    /// records, for pool creation).
    pub fn has_account_tail(self) -> bool {
        matches!(self, Self::Initialize | Self::InitializeWithPermission)
    }

    /// Where token_0's and token_1's transfer accounts sit in the fixed list.
    fn layouts(self) -> (LegLayout, LegLayout) {
        // (source, destination, authority, mint) of the token_0 and token_1 transfers.
        let (a, b) = match self {
            // owner pays: token account -> vault.
            Self::Deposit => ((4, 6, 0, 10), (5, 7, 0, 11)),
            // the pool authority pays: vault -> token account.
            Self::Withdraw => ((6, 4, 1, 10), (7, 5, 1, 11)),
            Self::CollectProtocolFee | Self::CollectFundFee => ((4, 8, 1, 6), (5, 9, 1, 7)),
            Self::CollectCreatorFee => ((4, 8, 1, 6), (5, 9, 1, 7)),
            Self::CollectCreatorFeePermissionless => ((4, 8, 2, 6), (5, 9, 2, 7)),
            Self::Initialize => ((7, 10, 0, 4), (8, 11, 0, 5)),
            Self::InitializeWithPermission => ((8, 11, 0, 5), (9, 12, 0, 6)),
        };
        let layout = |(source, destination, authority, mint)| LegLayout {
            source,
            destination,
            authority,
            mint,
        };
        (layout(a), layout(b))
    }
}

fn check_v1(instruction: &Instruction, op: CpmmPairOp) -> Result<(), FrameError> {
    if instruction.data.len() >= 8 && instruction.data[..8] == op.v2_discriminator() {
        return Err(FrameError::AlreadyFramed);
    }
    if instruction.data.len() != op.v1_data_len() || instruction.data[..8] != op.v1_discriminator()
    {
        return Err(FrameError::InvalidInstructionData);
    }
    let found = instruction.accounts.len();
    let fixed = op.fixed_accounts();
    if found < fixed || (found > fixed && !op.has_account_tail()) {
        return Err(FrameError::InvalidFixedAccountCount {
            expected: fixed,
            found,
        });
    }
    Ok(())
}

fn frame(
    op: CpmmPairOp,
    instruction: &mut Instruction,
    token_0: &LegHook,
    token_1: &LegHook,
) -> Result<FramedSwap, FrameError> {
    check_v1(instruction, op)?;
    let (layout_0, layout_1) = op.layouts();
    let (count_0, count_1) = validate_legs(
        &instruction.accounts,
        token_0,
        token_1,
        &layout_0,
        &layout_1,
    )?;

    instruction.data[..8].copy_from_slice(&op.v2_discriminator());
    instruction.data.extend_from_slice(&count_0.to_le_bytes());
    instruction.data.extend_from_slice(&count_1.to_le_bytes());

    // The slices go right after the fixed list; anything after it (support-mint records) follows
    // them, which is the order the program reads them in.
    let at = op.fixed_accounts();
    let tail: Vec<AccountMeta> = instruction.accounts.split_off(at);
    let start = instruction.accounts.len();
    if let Some(slice) = token_0.slice() {
        instruction.accounts.extend_from_slice(slice.metas());
    }
    let middle = instruction.accounts.len();
    if let Some(slice) = token_1.slice() {
        instruction.accounts.extend_from_slice(slice.metas());
    }
    let end = instruction.accounts.len();
    instruction.accounts.extend(tail);

    Ok(FramedSwap {
        abi: FramedAbi::CpmmPair(op),
        tick_array_count: 0,
        bitmap_count: 0,
        input_hook_accounts: count_0,
        output_hook_accounts: count_1,
        input_range: start..middle,
        output_range: middle..end,
    })
}

/// Convert a V1 two-token CPMM instruction to its `_v2`, appending token_0's slice then token_1's.
/// Both legs may be unhooked, which yields an explicit `_v2` with zero counts. The instruction is
/// only modified on success.
pub fn frame_cpmm_pair_v2(
    op: CpmmPairOp,
    instruction: &mut Instruction,
    token_0: &LegHook,
    token_1: &LegHook,
) -> Result<FramedSwap, FrameError> {
    frame(op, instruction, token_0, token_1)
}

/// Like [`frame_cpmm_pair_v2`], but if neither leg is hooked the instruction is validated and left
/// as the byte-identical V1 (`Ok(None)`).
pub fn frame_cpmm_pair_or_passthrough(
    op: CpmmPairOp,
    instruction: &mut Instruction,
    token_0: &LegHook,
    token_1: &LegHook,
) -> Result<Option<FramedSwap>, FrameError> {
    if token_0.is_hooked() || token_1.is_hooked() {
        return frame(op, instruction, token_0, token_1).map(Some);
    }
    check_v1(instruction, op)?;
    let (layout_0, layout_1) = op.layouts();
    validate_legs(
        &instruction.accounts,
        token_0,
        token_1,
        &layout_0,
        &layout_1,
    )?;
    Ok(None)
}
