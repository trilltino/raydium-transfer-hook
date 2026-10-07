//! Golden ABI fixtures for the pinned Raydium CPMM / CLMM instructions.
//!
//! The JSON files under `tests/golden/` are the committed ABI record: the
//! discriminator, instruction data, and every account with its role, signer
//! and writable flag, in order. Regenerate after an intentional ABI change with
//! `UPDATE_GOLDEN=1 cargo test -p transfer-hook-sdk golden`.

use std::{fmt::Write as _, path::PathBuf};

use solana_program::{hash::hash, instruction::Instruction, pubkey::Pubkey};
use spl_tlv_account_resolution::account::ExtraAccountMeta;

use crate::{
    abi::{
        anchor_instruction_discriminator, build_clmm_swap_v2, build_cpmm_swap_base_input_v1,
        ClmmSwapAccounts, ClmmSwapArgs, CpmmSwapAccounts, CLMM_SWAP_V2_DISCRIMINATOR,
        CLMM_SWAP_V3_DISCRIMINATOR, CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR,
        CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR,
    },
    error::LegRole,
    frame::{frame_clmm_swap_v3, frame_cpmm_swap_base_input_v2},
    resolve::{resolve_leg, ResolveOptions, SplTransferLeg},
    testing::MemoryChain,
};

const CPMM_ROLES: [&str; 13] = [
    "payer",
    "authority",
    "amm_config",
    "pool_state",
    "input_token_account",
    "output_token_account",
    "input_vault",
    "output_vault",
    "input_token_program",
    "output_token_program",
    "input_token_mint",
    "output_token_mint",
    "observation_state",
];

const CLMM_ROLES: [&str; 13] = [
    "payer",
    "amm_config",
    "pool_state",
    "input_token_account",
    "output_token_account",
    "input_vault",
    "output_vault",
    "observation_state",
    "token_program",
    "token_program_2022",
    "memo_program",
    "input_vault_mint",
    "output_vault_mint",
];

/// (signer, writable) per fixed account, transcribed from the pinned upstream
/// Anchor `Swap` and `SwapSingleV2` account structs.
const CPMM_FLAGS: [(bool, bool); 13] = [
    (true, false),
    (false, false),
    (false, false),
    (false, true),
    (false, true),
    (false, true),
    (false, true),
    (false, true),
    (false, false),
    (false, false),
    (false, false),
    (false, false),
    (false, true),
];

const CLMM_FLAGS: [(bool, bool); 13] = [
    (true, false),
    (false, false),
    (false, true),
    (false, true),
    (false, true),
    (false, true),
    (false, true),
    (false, true),
    (false, false),
    (false, false),
    (false, false),
    (false, false),
    (false, false),
];

fn key(byte: u8) -> Pubkey {
    Pubkey::new_from_array([byte; 32])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn render(
    name: &str,
    discriminator_name: &str,
    instruction: &Instruction,
    roles: &[String],
) -> String {
    assert_eq!(roles.len(), instruction.accounts.len());
    let mut out = String::new();
    out.push_str("{\n");
    let _ = writeln!(out, "  \"name\": \"{name}\",");
    let _ = writeln!(out, "  \"program_id\": \"{}\",", instruction.program_id);
    let _ = writeln!(out, "  \"discriminator_name\": \"{discriminator_name}\",");
    let _ = writeln!(
        out,
        "  \"discriminator_hex\": \"{}\",",
        hex(&instruction.data[..8])
    );
    let _ = writeln!(out, "  \"data_hex\": \"{}\",", hex(&instruction.data));
    out.push_str("  \"accounts\": [\n");
    for (index, (meta, role)) in instruction.accounts.iter().zip(roles).enumerate() {
        let comma = if index + 1 == roles.len() { "" } else { "," };
        let _ = writeln!(
            out,
            "    {{\"index\": {index}, \"role\": \"{role}\", \"pubkey\": \"{}\", \"signer\": {}, \"writable\": {}}}{comma}",
            meta.pubkey, meta.is_signer, meta.is_writable
        );
    }
    out.push_str("  ]\n}\n");
    out
}

fn golden_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(file)
}

fn assert_golden(file: &str, rendered: &str) {
    let path = golden_path(file);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, rendered).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("missing golden {}: {error}", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(
        rendered, expected,
        "golden {file} differs; rerun with UPDATE_GOLDEN=1 if the ABI change is intended"
    );
}

fn roles(fixed: &[&str], extra: Vec<String>) -> Vec<String> {
    fixed
        .iter()
        .map(|role| role.to_string())
        .chain(extra)
        .collect()
}

fn slice_roles(prefix: &str, len: usize) -> Vec<String> {
    (0..len)
        .map(|index| {
            if index + 2 == len {
                format!("{prefix}_hook_program")
            } else if index + 1 == len {
                format!("{prefix}_validation_list")
            } else {
                format!("{prefix}_hook_extra_{index}")
            }
        })
        .collect()
}

fn cpmm_accounts() -> CpmmSwapAccounts {
    CpmmSwapAccounts {
        payer: key(1),
        authority: key(2),
        amm_config: key(3),
        pool_state: key(4),
        input_token_account: key(5),
        output_token_account: key(6),
        input_vault: key(7),
        output_vault: key(8),
        input_token_program: key(9),
        output_token_program: key(10),
        input_token_mint: key(11),
        output_token_mint: key(12),
        observation_state: key(13),
    }
}

fn clmm_accounts() -> ClmmSwapAccounts {
    ClmmSwapAccounts {
        payer: key(1),
        amm_config: key(2),
        pool_state: key(3),
        input_token_account: key(4),
        output_token_account: key(5),
        input_vault: key(6),
        output_vault: key(7),
        observation_state: key(8),
        token_program: key(9),
        token_program_2022: key(10),
        memo_program: key(11),
        input_vault_mint: key(12),
        output_vault_mint: key(13),
    }
}

const CPMM_PROGRAM: Pubkey = Pubkey::new_from_array([0xC1; 32]);
const CLMM_PROGRAM: Pubkey = Pubkey::new_from_array([0xC2; 32]);

fn cpmm_v1() -> Instruction {
    build_cpmm_swap_base_input_v1(CPMM_PROGRAM, &cpmm_accounts(), 1_000_000, 900_000)
}

fn clmm_v2() -> Instruction {
    build_clmm_swap_v2(
        CLMM_PROGRAM,
        &clmm_accounts(),
        &[key(0x71), key(0x72)],
        Some(key(0x73)),
        ClmmSwapArgs {
            amount: 1_000_000,
            other_amount_threshold: 900_000,
            sqrt_price_limit_x64: 0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10,
            is_base_input: true,
        },
    )
}

/// A deterministic chain: mint `m` hooked to `hook`, one fixed extra account.
fn hooked_chain(mints: &[(Pubkey, Pubkey, Pubkey)]) -> MemoryChain {
    let mut chain = MemoryChain::new();
    for (mint, hook, extra) in mints {
        chain.add_hooked_mint(
            *mint,
            *hook,
            None,
            &[ExtraAccountMeta::new_with_pubkey(extra, false, false).unwrap()],
        );
    }
    chain
}

#[test]
fn discriminators_are_sha256_of_the_anchor_global_names() {
    for (name, constant) in [
        ("swap_base_input", CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR),
        ("swap_base_input_v2", CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR),
        ("swap_v2", CLMM_SWAP_V2_DISCRIMINATOR),
        ("swap_v3", CLMM_SWAP_V3_DISCRIMINATOR),
    ] {
        let digest = hash(format!("global:{name}").as_bytes()).to_bytes();
        assert_eq!(constant, digest[..8], "{name}");
        assert_eq!(anchor_instruction_discriminator(name), constant, "{name}");
    }
}

#[test]
fn v1_fixed_accounts_match_the_pinned_upstream_order_and_flags() {
    let cpmm = cpmm_v1();
    assert_eq!(cpmm.accounts.len(), 13);
    for (index, meta) in cpmm.accounts.iter().enumerate() {
        assert_eq!(
            (meta.is_signer, meta.is_writable),
            CPMM_FLAGS[index],
            "cpmm {}",
            CPMM_ROLES[index]
        );
        assert_eq!(meta.pubkey, key(index as u8 + 1));
    }
    let clmm = clmm_v2();
    assert_eq!(clmm.accounts.len(), 13 + 3);
    for (index, meta) in clmm.accounts[..13].iter().enumerate() {
        assert_eq!(
            (meta.is_signer, meta.is_writable),
            CLMM_FLAGS[index],
            "clmm {}",
            CLMM_ROLES[index]
        );
    }
    // Tick arrays then the bitmap extension, all writable non-signers.
    assert!(clmm.accounts[13..]
        .iter()
        .all(|meta| !meta.is_signer && meta.is_writable));
    assert_eq!(clmm.data.len(), 41);
}

#[test]
fn cpmm_swap_base_input_v1_golden() {
    let instruction = cpmm_v1();
    assert_eq!(instruction.data.len(), 24);
    let roles = roles(&CPMM_ROLES, Vec::new());
    assert_golden(
        "cpmm_swap_base_input_v1.json",
        &render(
            "cpmm_swap_base_input_v1",
            "swap_base_input",
            &instruction,
            &roles,
        ),
    );
}

#[test]
fn clmm_swap_v2_golden() {
    let instruction = clmm_v2();
    let roles = roles(
        &CLMM_ROLES,
        vec![
            "tick_array_0".into(),
            "tick_array_1".into(),
            "bitmap_extension".into(),
        ],
    );
    assert_golden(
        "clmm_swap_v2.json",
        &render("clmm_swap_v2", "swap_v2", &instruction, &roles),
    );
}

#[tokio::test]
async fn cpmm_swap_base_input_v2_golden_and_fixed_accounts_unchanged_by_framing() {
    let accounts = cpmm_accounts();
    let (hook_in, hook_out) = (key(0x91), key(0x92));
    let chain = hooked_chain(&[
        (accounts.input_token_mint, hook_in, key(0xA1)),
        (accounts.output_token_mint, hook_out, key(0xA2)),
    ]);
    let options = ResolveOptions::default();
    let input = resolve_leg(
        LegRole::Input,
        SplTransferLeg {
            source: accounts.input_token_account,
            mint: accounts.input_token_mint,
            destination: accounts.input_vault,
            authority: accounts.payer,
            amount: 1_000_000,
        },
        &options,
        chain.fetcher(),
    )
    .await
    .unwrap();
    let output = resolve_leg(
        LegRole::Output,
        SplTransferLeg {
            source: accounts.output_vault,
            mint: accounts.output_token_mint,
            destination: accounts.output_token_account,
            authority: accounts.authority,
            amount: 900_000,
        },
        &options,
        chain.fetcher(),
    )
    .await
    .unwrap();

    let v1 = cpmm_v1();
    let mut v2 = v1.clone();
    let framed = frame_cpmm_swap_base_input_v2(&mut v2, &input, &output).unwrap();

    // Framing changes only the discriminator, appends the two counts, and
    // appends the slices: every fixed account and flag is byte-identical.
    assert_eq!(v2.accounts[..13], v1.accounts[..]);
    assert_eq!(v2.program_id, v1.program_id);
    assert_eq!(v2.data[8..24], v1.data[8..24]);
    assert_eq!(v2.data[..8], CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR);
    assert_eq!(v2.data[24..], [3, 0, 3, 0]);
    assert_eq!(framed.input_range, 13..16);

    let roles = roles(
        &CPMM_ROLES,
        [slice_roles("input", 3), slice_roles("output", 3)].concat(),
    );
    assert_golden(
        "cpmm_swap_base_input_v2.json",
        &render("cpmm_swap_base_input_v2", "swap_base_input_v2", &v2, &roles),
    );
}

#[tokio::test]
async fn clmm_swap_v3_golden_and_fixed_accounts_unchanged_by_framing() {
    let accounts = clmm_accounts();
    let (hook_in, hook_out) = (key(0x93), key(0x94));
    let chain = hooked_chain(&[
        (accounts.input_vault_mint, hook_in, key(0xB1)),
        (accounts.output_vault_mint, hook_out, key(0xB2)),
    ]);
    let options = ResolveOptions::default();
    let input = resolve_leg(
        LegRole::Input,
        SplTransferLeg {
            source: accounts.input_token_account,
            mint: accounts.input_vault_mint,
            destination: accounts.input_vault,
            authority: accounts.payer,
            amount: 1_000_000,
        },
        &options,
        chain.fetcher(),
    )
    .await
    .unwrap();
    let output = resolve_leg(
        LegRole::Output,
        SplTransferLeg {
            source: accounts.output_vault,
            mint: accounts.output_vault_mint,
            destination: accounts.output_token_account,
            authority: accounts.pool_state,
            amount: 900_000,
        },
        &options,
        chain.fetcher(),
    )
    .await
    .unwrap();

    let v2 = clmm_v2();
    let mut v3 = v2.clone();
    let framed = frame_clmm_swap_v3(&mut v3, 2, 1, &input, &output).unwrap();

    assert_eq!(v3.accounts[..16], v2.accounts[..]);
    assert_eq!(v3.program_id, v2.program_id);
    assert_eq!(v3.data[8..41], v2.data[8..41]);
    assert_eq!(v3.data[..8], CLMM_SWAP_V3_DISCRIMINATOR);
    assert_eq!(v3.data[41..], [2, 0, 1, 0, 3, 0, 3, 0]);
    assert_eq!(framed.output_range, 19..22);

    let roles = roles(
        &CLMM_ROLES,
        [
            vec![
                "tick_array_0".to_string(),
                "tick_array_1".to_string(),
                "bitmap_extension".to_string(),
            ],
            slice_roles("input", 3),
            slice_roles("output", 3),
        ]
        .concat(),
    );
    assert_golden(
        "clmm_swap_v3.json",
        &render("clmm_swap_v3", "swap_v3", &v3, &roles),
    );
}
