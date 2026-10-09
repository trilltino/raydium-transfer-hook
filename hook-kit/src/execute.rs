//! The `Execute` prelude: every check a hook must make before it looks at its own rule.

use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

use crate::{
    accounts::{validation_list_address, LIST_HEADER_LEN, META_LEN},
    error::KitError,
    mint::{read_hook_mint, require_hook_program, HookMint},
    token::{read_token_account, TokenView},
};

/// The SPL `Execute` instruction discriminator.
pub const EXECUTE_DISCRIMINATOR: [u8; 8] = [105, 37, 101, 197, 75, 251, 102, 26];

/// `Execute` instruction data: the discriminator and a little-endian `u64` amount.
const EXECUTE_DATA_LEN: usize = 8 + 8;

/// Accounts every `Execute` carries before the hook's extras: source, mint, destination, owner,
/// validation list.
pub const FIXED_ACCOUNTS: usize = 5;

/// The validated inputs of an `Execute` call.
pub struct ExecuteCtx<'a, 'info> {
    pub amount: u64,
    pub source: &'a AccountInfo<'info>,
    pub mint: &'a AccountInfo<'info>,
    pub destination: &'a AccountInfo<'info>,
    pub owner: &'a AccountInfo<'info>,
    /// The hook-specific accounts, in the order the validation list declares them.
    pub extras: &'a [AccountInfo<'info>],
    pub hook_mint: HookMint,
    /// Post-transfer views (Token-2022 moves the tokens before it calls the hook).
    pub source_view: TokenView,
    pub destination_view: TokenView,
}

/// Whether `instruction_data` is an `Execute` call, and its transfer amount.
///
/// # Errors
/// `InvalidInstructionData` unless the data is exactly the discriminator and a `u64`.
pub fn parse_execute_amount(instruction_data: &[u8]) -> Result<u64, ProgramError> {
    if instruction_data.len() != EXECUTE_DATA_LEN || instruction_data[..8] != EXECUTE_DISCRIMINATOR
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let bytes: [u8; 8] = instruction_data[8..]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    Ok(u64::from_le_bytes(bytes))
}

/// Validate an `Execute` call for a hook whose validation list is `canonical_list` (see
/// [`crate::canonical_list`]).
///
/// Checks, in order: the instruction data and the exact account count the list implies; that none
/// of source, mint, destination and validation list is writable (Token-2022 builds the call with
/// them read-only); the mint is Token-2022, carries a TransferHook extension and points at
/// `program_id`; both token accounts belong to the mint and carry the `transferring` flag (only
/// Token-2022 sets it, so a direct call is refused); the validation list is the canonical
/// address, owned by `program_id`, and byte for byte `canonical_list`.
///
/// The hook must verify the extra accounts it relies on. In particular, an uninitialized PDA is
/// system-owned and has no contents to identify it, so a rule that treats it as optional must
/// check its address before accepting it as absent.
///
/// # Errors
/// A [`KitError`] or a `ProgramError` for the first check that fails.
pub fn execute_prelude<'a, 'info>(
    program_id: &Pubkey,
    accounts: &'a [AccountInfo<'info>],
    instruction_data: &[u8],
    canonical_list: &[u8],
) -> Result<ExecuteCtx<'a, 'info>, ProgramError> {
    let amount = parse_execute_amount(instruction_data)?;
    let extra_count = canonical_list
        .len()
        .checked_sub(LIST_HEADER_LEN)
        .map(|metas| metas / META_LEN)
        .ok_or(KitError::InvalidValidationList)?;
    let [source, mint, destination, owner, validation_list, extras @ ..] = accounts else {
        return Err(KitError::WrongAccountCount.into());
    };
    if extras.len() != extra_count {
        return Err(KitError::WrongAccountCount.into());
    }
    // The owner slot is exempt: on a forged top-level call its writability is the transaction's
    // flag for the fee payer, and such calls are refused by the `transferring` check anyway.
    if [source, mint, destination, validation_list]
        .iter()
        .any(|account| account.is_writable)
    {
        return Err(KitError::WrongAccountCount.into());
    }

    let hook_mint = read_hook_mint(mint)?;
    require_hook_program(&hook_mint, program_id)?;

    let source_view = read_token_account(source)?;
    let destination_view = read_token_account(destination)?;
    if source_view.mint != *mint.key || destination_view.mint != *mint.key {
        return Err(KitError::TokenAccountMismatch.into());
    }
    if !source_view.transferring || !destination_view.transferring {
        return Err(KitError::NotDirectInvocation.into());
    }

    let (expected_list, _) = validation_list_address(mint.key, program_id);
    if validation_list.key != &expected_list
        || validation_list.owner != program_id
        || validation_list.try_borrow_data()?.as_ref() != canonical_list
    {
        return Err(KitError::InvalidValidationList.into());
    }

    Ok(ExecuteCtx {
        amount,
        source,
        mint,
        destination,
        owner,
        extras,
        hook_mint,
        source_view,
        destination_view,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_data_must_be_the_discriminator_and_a_u64() {
        let mut data = EXECUTE_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&0x0102_0304_0506_0708u64.to_le_bytes());
        assert_eq!(parse_execute_amount(&data), Ok(0x0102_0304_0506_0708));
        assert_eq!(
            parse_execute_amount(&data[..15]),
            Err(ProgramError::InvalidInstructionData)
        );
        data.push(0);
        assert_eq!(
            parse_execute_amount(&data),
            Err(ProgramError::InvalidInstructionData)
        );
        data.pop();
        data[0] ^= 1;
        assert_eq!(
            parse_execute_amount(&data),
            Err(ProgramError::InvalidInstructionData)
        );
        assert_eq!(
            parse_execute_amount(&[]),
            Err(ProgramError::InvalidInstructionData)
        );
    }

    #[test]
    fn the_discriminator_matches_the_spl_interface() {
        use spl_transfer_hook_interface::instruction::TransferHookInstruction;
        assert_eq!(
            TransferHookInstruction::Execute { amount: 1 }.pack()[..8],
            EXECUTE_DISCRIMINATOR
        );
    }

    // ---- the whole prelude, on hand-built accounts --------------------------------------

    use crate::{
        accounts::{canonical_list, list_len, seeded_meta},
        test_util::{hooked_account, hooked_mint},
    };

    /// Accounts in `Execute` order: source, mint, destination, owner, validation list, one extra.
    struct Fixture {
        program: Pubkey,
        keys: [Pubkey; 6],
        owners: [Pubkey; 6],
        datas: [Vec<u8>; 6],
        writable: [bool; 6],
        instruction: Vec<u8>,
        list: [u8; list_len(1)],
        count: usize,
    }

    fn fixture() -> Fixture {
        let program = Pubkey::new_unique();
        let (mint, holder) = (Pubkey::new_unique(), Pubkey::new_unique());
        let list = canonical_list([seeded_meta(b"config", 1, false)]);
        let token_2022 = spl_token_2022::id();
        let mut instruction = EXECUTE_DISCRIMINATOR.to_vec();
        instruction.extend_from_slice(&7u64.to_le_bytes());
        Fixture {
            program,
            keys: [
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                holder,
                validation_list_address(&mint, &program).0,
                Pubkey::new_unique(),
            ],
            owners: [
                token_2022,
                token_2022,
                token_2022,
                solana_program::system_program::id(),
                program,
                program,
            ],
            datas: [
                hooked_account(mint, holder, 10, true),
                hooked_mint(None, Some(program), None, 100),
                hooked_account(mint, Pubkey::new_unique(), 3, true),
                vec![],
                list.to_vec(),
                vec![],
            ],
            writable: [false; 6],
            instruction,
            list,
            count: 6,
        }
    }

    fn run(f: &mut Fixture) -> Result<u64, ProgramError> {
        let mut lamports = [1_000_000u64; 6];
        let infos: Vec<AccountInfo> = f
            .keys
            .iter()
            .zip(&f.owners)
            .zip(f.datas.iter_mut())
            .zip(lamports.iter_mut())
            .zip(&f.writable)
            .take(f.count)
            .map(|((((key, owner), data), lamports), writable)| {
                AccountInfo::new(key, false, *writable, lamports, data, owner, false, 0)
            })
            .collect();
        execute_prelude(&f.program, &infos, &f.instruction, &f.list).map(|ctx| {
            assert_eq!(ctx.extras.len(), 1);
            assert_eq!(ctx.source_view.amount, 10);
            assert_eq!(ctx.destination_view.amount, 3);
            ctx.amount
        })
    }

    fn refused(error: KitError) -> Result<u64, ProgramError> {
        Err(ProgramError::Custom(error.code()))
    }

    #[test]
    fn a_genuine_execute_call_passes_and_carries_its_amount() {
        assert_eq!(run(&mut fixture()), Ok(7));
    }

    #[test]
    fn the_account_count_must_be_exactly_what_the_list_declares() {
        let mut short = fixture();
        short.count = 5;
        assert_eq!(run(&mut short), refused(KitError::WrongAccountCount));
        let mut tiny = fixture();
        tiny.count = 3;
        assert_eq!(run(&mut tiny), refused(KitError::WrongAccountCount));
    }

    #[test]
    fn source_mint_destination_and_list_must_be_read_only() {
        for index in [0, 1, 2, 4] {
            let mut f = fixture();
            f.writable[index] = true;
            assert_eq!(
                run(&mut f),
                refused(KitError::WrongAccountCount),
                "account {index}"
            );
        }
        // The owner slot and the extra account are not checked here.
        let mut f = fixture();
        f.writable[3] = true;
        f.writable[5] = true;
        assert_eq!(run(&mut f), Ok(7));
    }

    #[test]
    fn a_call_outside_a_transfer_is_refused() {
        for index in [0, 2] {
            let mut f = fixture();
            let mint = f.keys[1];
            f.datas[index] = hooked_account(mint, Pubkey::new_unique(), 10, false);
            assert_eq!(
                run(&mut f),
                refused(KitError::NotDirectInvocation),
                "account {index}"
            );
        }
    }

    #[test]
    fn the_mint_must_point_at_this_program_and_own_both_token_accounts() {
        let mut other_hook = fixture();
        other_hook.datas[1] = hooked_mint(None, Some(Pubkey::new_unique()), None, 100);
        assert_eq!(run(&mut other_hook), refused(KitError::MintHookMismatch));

        let mut foreign_account = fixture();
        foreign_account.datas[2] =
            hooked_account(Pubkey::new_unique(), Pubkey::new_unique(), 3, true);
        assert_eq!(
            run(&mut foreign_account),
            refused(KitError::TokenAccountMismatch)
        );

        let mut not_token_2022 = fixture();
        not_token_2022.owners[1] = Pubkey::new_unique();
        assert_eq!(
            run(&mut not_token_2022),
            refused(KitError::MintNotToken2022)
        );
    }

    #[test]
    fn the_validation_list_must_be_the_canonical_one_byte_for_byte() {
        let mut wrong_key = fixture();
        wrong_key.keys[4] = Pubkey::new_unique();
        assert_eq!(
            run(&mut wrong_key),
            refused(KitError::InvalidValidationList)
        );

        let mut wrong_owner = fixture();
        wrong_owner.owners[4] = Pubkey::new_unique();
        assert_eq!(
            run(&mut wrong_owner),
            refused(KitError::InvalidValidationList)
        );

        for byte in 0..list_len(1) {
            let mut f = fixture();
            f.datas[4][byte] ^= 1;
            assert_eq!(
                run(&mut f),
                refused(KitError::InvalidValidationList),
                "byte {byte}"
            );
        }
        let mut truncated = fixture();
        truncated.datas[4].pop();
        assert_eq!(
            run(&mut truncated),
            refused(KitError::InvalidValidationList)
        );
        let mut padded = fixture();
        padded.datas[4].push(0);
        assert_eq!(run(&mut padded), refused(KitError::InvalidValidationList));
    }
}
