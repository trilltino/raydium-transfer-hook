//! Creating program-derived accounts and the canonical validation list.
//!
//! A hook's validation list is fixed by the rule: every mint of the same hook gets the same
//! bytes. [`canonical_list`] builds those bytes at compile time, `Initialize` writes them with
//! [`create_validation_list`], and [`crate::execute_prelude`] compares them byte for byte on every
//! transfer. That is cheaper and stricter than resolving the list through the SPL crate on
//! each `Execute`, and it cannot panic on a corrupt list.

use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};
use spl_transfer_hook_interface::{
    collect_extra_account_metas_signer_seeds, get_extra_account_metas_address_and_bump_seed,
};

use crate::{error::KitError, execute::EXECUTE_DISCRIMINATOR};

/// Bytes of one `ExtraAccountMeta` in a validation list.
pub const META_LEN: usize = 35;
/// Bytes before the first meta: TLV type (8) + TLV length (4) + entry count (4).
pub const LIST_HEADER_LEN: usize = 16;

/// The length of a validation list that declares `extras` extra accounts.
pub const fn list_len(extras: usize) -> usize {
    LIST_HEADER_LEN + META_LEN * extras
}

/// The canonical validation-list address of `mint` under `program_id`.
pub fn validation_list_address(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    get_extra_account_metas_address_and_bump_seed(mint, program_id)
}

/// An extra account that is a PDA of the hook program with seeds `[literal, accounts[index]]`
/// (for example `["config", mint]` with `account_index = 1`). `literal` is at most 28 bytes.
pub const fn seeded_meta(literal: &[u8], account_index: u8, writable: bool) -> [u8; META_LEN] {
    assert!(
        literal.len() <= 28,
        "seed literal too long for a packed meta"
    );
    let mut meta = [0u8; META_LEN];
    meta[0] = 1; // discriminator: seeds of this program
    meta[1] = 1; // seed kind: literal
    meta[2] = literal.len() as u8;
    let mut i = 0;
    while i < literal.len() {
        meta[3 + i] = literal[i];
        i += 1;
    }
    meta[3 + literal.len()] = 3; // seed kind: the key of an instruction account
    meta[4 + literal.len()] = account_index;
    // bytes up to 33 stay zero (padding); then is_signer = 0
    meta[34] = writable as u8;
    meta
}

/// An extra account at a fixed address (for example a sysvar). Never a signer.
pub const fn pubkey_meta(address: &[u8; 32], writable: bool) -> [u8; META_LEN] {
    let mut meta = [0u8; META_LEN];
    // discriminator 0: a plain address
    let mut i = 0;
    while i < 32 {
        meta[1 + i] = address[i];
        i += 1;
    }
    meta[34] = writable as u8;
    meta
}

/// The validation-list bytes for `E` extra accounts. `N` must be [`list_len`]`(E)`; a mismatch
/// fails to compile when used in a `const`.
pub const fn canonical_list<const E: usize, const N: usize>(metas: [[u8; META_LEN]; E]) -> [u8; N] {
    assert!(N == list_len(E), "N must be list_len(E)");
    let mut list = [0u8; N];
    let mut i = 0;
    while i < 8 {
        list[i] = EXECUTE_DISCRIMINATOR[i];
        i += 1;
    }
    // TLV length: the entry count (4) plus the metas.
    let tlv_len = ((4 + META_LEN * E) as u32).to_le_bytes();
    let count = (E as u32).to_le_bytes();
    let mut i = 0;
    while i < 4 {
        list[8 + i] = tlv_len[i];
        list[12 + i] = count[i];
        i += 1;
    }
    let mut m = 0;
    while m < E {
        let mut b = 0;
        while b < META_LEN {
            list[LIST_HEADER_LEN + m * META_LEN + b] = metas[m][b];
            b += 1;
        }
        m += 1;
    }
    list
}

/// Create the PDA at `account` (seeds `seeds`, including the bump) owned by `program_id`.
///
/// Survives a pre-funded address (anyone can send lamports to a deterministic PDA): it then tops
/// up, allocates and assigns instead of failing.
pub fn create_pda<'a>(
    payer: &AccountInfo<'a>,
    account: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    program_id: &Pubkey,
    space: usize,
    seeds: &[&[u8]],
) -> ProgramResult {
    if account.owner != &solana_program::system_program::id() || !account.data_is_empty() {
        return Err(KitError::AlreadyInitialized.into());
    }
    let required = Rent::get()?.minimum_balance(space);
    let space_u64 = u64::try_from(space).map_err(|_| ProgramError::InvalidArgument)?;
    let current = account.lamports();
    if current == 0 {
        invoke_signed(
            &system_instruction::create_account(
                payer.key,
                account.key,
                required,
                space_u64,
                program_id,
            ),
            &[payer.clone(), account.clone(), system_program.clone()],
            &[seeds],
        )
    } else {
        let missing = required.saturating_sub(current);
        if missing > 0 {
            invoke(
                &system_instruction::transfer(payer.key, account.key, missing),
                &[payer.clone(), account.clone(), system_program.clone()],
            )?;
        }
        invoke_signed(
            &system_instruction::allocate(account.key, space_u64),
            &[account.clone(), system_program.clone()],
            &[seeds],
        )?;
        invoke_signed(
            &system_instruction::assign(account.key, program_id),
            &[account.clone(), system_program.clone()],
            &[seeds],
        )
    }
}

/// Create the validation list at its canonical address and write `canonical` (a
/// [`canonical_list`]) into it.
pub fn create_validation_list<'a>(
    payer: &AccountInfo<'a>,
    list: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    mint: &Pubkey,
    program_id: &Pubkey,
    canonical: &[u8],
) -> ProgramResult {
    let (expected, bump) = validation_list_address(mint, program_id);
    if list.key != &expected {
        return Err(KitError::InvalidValidationList.into());
    }
    let bump_seed = [bump];
    create_pda(
        payer,
        list,
        system_program,
        program_id,
        canonical.len(),
        &collect_extra_account_metas_signer_seeds(mint, &bump_seed),
    )?;
    list.try_borrow_mut_data()?.copy_from_slice(canonical);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_program::sysvar;
    use spl_tlv_account_resolution::{
        account::ExtraAccountMeta, seeds::Seed, state::ExtraAccountMetaList,
    };
    use spl_transfer_hook_interface::instruction::ExecuteInstruction;

    /// What the SPL crate writes for `metas`.
    fn spl_list(metas: &[ExtraAccountMeta]) -> Vec<u8> {
        let mut data = vec![0u8; ExtraAccountMetaList::size_of(metas.len()).unwrap()];
        ExtraAccountMetaList::init::<ExecuteInstruction>(&mut data, metas).unwrap();
        data
    }

    fn seeded(literal: &[u8], index: u8, writable: bool) -> ExtraAccountMeta {
        ExtraAccountMeta::new_with_seeds(
            &[
                Seed::Literal {
                    bytes: literal.to_vec(),
                },
                Seed::AccountKey { index },
            ],
            false,
            writable,
        )
        .unwrap()
    }

    #[test]
    fn one_seeded_meta_matches_the_spl_crate() {
        const LIST: [u8; list_len(1)] = canonical_list([seeded_meta(b"config", 1, false)]);
        assert_eq!(LIST.to_vec(), spl_list(&[seeded(b"config", 1, false)]));
    }

    #[test]
    fn seeded_metas_match_for_any_literal_index_and_writability() {
        for literal in [&b"a"[..], b"counter", b"reward-vault", &[7u8; 28][..]] {
            for index in [0u8, 1, 2, 4] {
                for writable in [false, true] {
                    let list: [u8; list_len(1)] =
                        canonical_list([seeded_meta(literal, index, writable)]);
                    assert_eq!(list.to_vec(), spl_list(&[seeded(literal, index, writable)]));
                }
            }
        }
    }

    #[test]
    fn three_mixed_metas_match_the_spl_crate() {
        const LIST: [u8; list_len(3)] = canonical_list([
            seeded_meta(b"rewards", 1, true),
            seeded_meta(b"holder", 0, true),
            pubkey_meta(&sysvar::instructions::ID.to_bytes(), false),
        ]);
        let expected = spl_list(&[
            seeded(b"rewards", 1, true),
            seeded(b"holder", 0, true),
            ExtraAccountMeta::new_with_pubkey(&sysvar::instructions::id(), false, false).unwrap(),
        ]);
        assert_eq!(LIST.to_vec(), expected);
    }

    #[test]
    fn list_len_matches_the_spl_size() {
        for extras in 0..6 {
            assert_eq!(
                list_len(extras),
                ExtraAccountMetaList::size_of(extras).unwrap()
            );
        }
    }

    #[test]
    fn the_validation_list_address_is_the_spl_one() {
        let (mint, program) = (Pubkey::new_unique(), Pubkey::new_unique());
        let (address, bump) = validation_list_address(&mint, &program);
        let seeds: [&[u8]; 3] = [b"extra-account-metas", mint.as_ref(), &[bump]];
        assert_eq!(
            Pubkey::create_program_address(&seeds, &program),
            Ok(address)
        );
        assert_eq!(
            collect_extra_account_metas_signer_seeds(&mint, &[bump]),
            seeds
        );
    }
}
