//! Transport readiness of a hooked mint: the facts that decide whether a transfer can reach the
//! hook at all.
//!
//! This is deliberately **only** transport. It can tell that the mint points at an executable
//! program and that a validation list exists, is owned by that program and has the Execute shape.
//! It cannot tell whether the hook's own settings were initialised, whether its rule is sensible,
//! whether it can freeze transfers on purpose, or who controls it. Those are business readiness;
//! see `docs/security.md`. Keeping the two apart is the point of the API.

use solana_program::{bpf_loader_upgradeable, pubkey::Pubkey};
use spl_token_2022::{
    extension::{
        transfer_hook::{get_program_id, TransferHook},
        BaseStateWithExtensions, StateWithExtensions,
    },
    state::Mint,
};

use crate::chain::{DriverError, Reader, Result};

/// The SPL `Execute` discriminator, which is also the type tag of the validation list's TLV entry.
const EXECUTE_DISCRIMINATOR: [u8; 8] = [105, 37, 101, 197, 75, 251, 102, 26];
/// Size of one `ExtraAccountMeta` in the validation list.
const EXTRA_ACCOUNT_META_LEN: usize = 35;

/// Who can replace a hook program's code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpgradeInfo {
    /// Deployed through the upgradeable loader, and the authority has been revoked.
    Immutable,
    /// Deployed through the upgradeable loader; this key can replace the code of every mint.
    Authority(Pubkey),
    /// Not an upgradeable-loader program (the loader that owns it is given), or its program-data
    /// account could not be read.
    NotApplicable(Pubkey),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramFacts {
    pub address: Pubkey,
    pub exists: bool,
    pub executable: bool,
    pub loader: Option<Pubkey>,
    pub upgrade: Option<UpgradeInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationListFacts {
    pub address: Pubkey,
    pub exists: bool,
    pub owned_by_hook: bool,
    /// The TLV entry has the Execute tag and a consistent length.
    pub has_execute_shape: bool,
    /// How many extra accounts the list declares (the hook's `N`).
    pub extra_accounts: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Readiness {
    pub mint: Pubkey,
    pub mint_exists: bool,
    pub token_2022: bool,
    pub has_hook_extension: bool,
    pub hook_program: Option<Pubkey>,
    /// The mint's TransferHook extension authority; `None` once revoked.
    pub hook_authority: Option<Pubkey>,
    pub program: Option<ProgramFacts>,
    pub validation_list: Option<ValidationListFacts>,
}

impl Readiness {
    /// Everything that stops a transfer from reaching the hook, one line each. Empty means the
    /// transport is ready. (It says nothing about whether the hook will accept the transfer.)
    pub fn transport_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !self.mint_exists {
            problems.push("the mint account does not exist".to_string());
            return problems;
        }
        if !self.token_2022 {
            problems.push("not a Token-2022 mint: no Transfer Hook is possible".into());
            return problems;
        }
        if !self.has_hook_extension {
            problems.push("the mint has no TransferHook extension".into());
            return problems;
        }
        let Some(program) = &self.hook_program else {
            problems.push("the TransferHook extension has no program set".into());
            return problems;
        };
        match &self.program {
            Some(p) if !p.exists => problems.push(format!("hook program {program} does not exist")),
            Some(p) if !p.executable => {
                problems.push(format!("hook program {program} is not executable"))
            }
            _ => {}
        }
        match &self.validation_list {
            Some(list) if !list.exists => problems.push(format!(
                "validation list {} does not exist: every transfer will fail",
                list.address
            )),
            Some(list) if !list.owned_by_hook => problems.push(format!(
                "validation list {} is not owned by the hook program",
                list.address
            )),
            Some(list) if !list.has_execute_shape => problems.push(format!(
                "validation list {} does not have the Execute shape",
                list.address
            )),
            _ => {}
        }
        problems
    }

    pub fn is_transport_ready(&self) -> bool {
        self.transport_problems().is_empty()
    }
}

fn upgrade_info(loader: Pubkey, program_data: &[u8], programdata: Option<&[u8]>) -> UpgradeInfo {
    if loader != bpf_loader_upgradeable::id() || program_data.len() < 36 {
        return UpgradeInfo::NotApplicable(loader);
    }
    let Some(data) = programdata else {
        return UpgradeInfo::NotApplicable(loader);
    };
    // ProgramData: tag (4), slot (8), Option<Pubkey> (1 + 32).
    match data.get(12) {
        Some(0) => UpgradeInfo::Immutable,
        Some(1) if data.len() >= 45 => match Pubkey::try_from(&data[13..45]) {
            Ok(authority) => UpgradeInfo::Authority(authority),
            Err(_) => UpgradeInfo::NotApplicable(loader),
        },
        _ => UpgradeInfo::NotApplicable(loader),
    }
}

/// Parse a validation list's TLV entry: type tag (8), length (4), then a `u32` count and that many
/// 35-byte metas. Returns the number of extra accounts if the shape is consistent.
fn execute_list_extras(data: &[u8]) -> Option<usize> {
    if data.len() < 16 || data[..8] != EXECUTE_DISCRIMINATOR {
        return None;
    }
    let length = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;
    let count = u32::from_le_bytes(data[12..16].try_into().ok()?) as usize;
    let consistent = length == 4 + count * EXTRA_ACCOUNT_META_LEN && data.len() >= 12 + length;
    consistent.then_some(count)
}

/// Whether `program` exists and is executable, which loader owns it, and who can upgrade it.
pub async fn inspect_program(reader: &Reader, program: Pubkey) -> Result<ProgramFacts> {
    let read = |key: Pubkey| {
        let reader = reader.clone();
        async move { reader(key).await }
    };
    Ok(match read(program).await? {
        None => ProgramFacts {
            address: program,
            exists: false,
            executable: false,
            loader: None,
            upgrade: None,
        },
        Some(p) => {
            let programdata = if p.owner == bpf_loader_upgradeable::id() && p.data.len() >= 36 {
                match Pubkey::try_from(&p.data[4..36]) {
                    Ok(address) => read(address).await?.map(|a| a.data),
                    Err(_) => None,
                }
            } else {
                None
            };
            ProgramFacts {
                address: program,
                exists: true,
                executable: p.executable,
                loader: Some(p.owner),
                upgrade: Some(upgrade_info(p.owner, &p.data, programdata.as_deref())),
            }
        }
    })
}

/// Read the transport facts of `mint` through `reader`.
pub async fn inspect_readiness(reader: &Reader, mint: Pubkey) -> Result<Readiness> {
    let read = |key: Pubkey| {
        let reader = reader.clone();
        async move { reader(key).await }
    };
    let mut readiness = Readiness {
        mint,
        mint_exists: false,
        token_2022: false,
        has_hook_extension: false,
        hook_program: None,
        hook_authority: None,
        program: None,
        validation_list: None,
    };
    let Some(account) = read(mint).await? else {
        return Ok(readiness);
    };
    readiness.mint_exists = true;
    readiness.token_2022 = account.owner == spl_token_2022::id();
    if !readiness.token_2022 {
        return Ok(readiness);
    }
    let state = StateWithExtensions::<Mint>::unpack(&account.data)
        .map_err(|e| DriverError::new(format!("the mint does not unpack: {e}")))?;
    let Ok(extension) = state.get_extension::<TransferHook>() else {
        return Ok(readiness);
    };
    readiness.has_hook_extension = true;
    readiness.hook_authority = Option::<Pubkey>::from(extension.authority);
    let Some(hook) = get_program_id(&state) else {
        return Ok(readiness);
    };
    readiness.hook_program = Some(hook);

    readiness.program = Some(inspect_program(reader, hook).await?);

    let list_address = spl_transfer_hook_interface::get_extra_account_metas_address(&mint, &hook);
    readiness.validation_list = Some(match read(list_address).await? {
        None => ValidationListFacts {
            address: list_address,
            exists: false,
            owned_by_hook: false,
            has_execute_shape: false,
            extra_accounts: None,
        },
        Some(list) => {
            let extras = execute_list_extras(&list.data);
            ValidationListFacts {
                address: list_address,
                exists: true,
                owned_by_hook: list.owner == hook,
                has_execute_shape: extras.is_some(),
                extra_accounts: extras,
            }
        }
    });
    Ok(readiness)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list_with(count: u32) -> Vec<u8> {
        let mut data = EXECUTE_DISCRIMINATOR.to_vec();
        let length = 4 + count as usize * EXTRA_ACCOUNT_META_LEN;
        data.extend_from_slice(&(length as u32).to_le_bytes());
        data.extend_from_slice(&count.to_le_bytes());
        data.extend(vec![0u8; count as usize * EXTRA_ACCOUNT_META_LEN]);
        data
    }

    #[test]
    fn a_consistent_execute_list_reports_its_extra_accounts() {
        assert_eq!(execute_list_extras(&list_with(0)), Some(0));
        assert_eq!(execute_list_extras(&list_with(3)), Some(3));
    }

    #[test]
    fn a_wrong_tag_length_or_truncation_is_not_an_execute_list() {
        let mut wrong_tag = list_with(1);
        wrong_tag[0] ^= 1;
        assert_eq!(execute_list_extras(&wrong_tag), None);
        let mut wrong_length = list_with(1);
        wrong_length[8] += 1;
        assert_eq!(execute_list_extras(&wrong_length), None);
        let truncated = list_with(2);
        assert_eq!(execute_list_extras(&truncated[..truncated.len() - 1]), None);
        assert_eq!(execute_list_extras(&[]), None);
    }

    #[test]
    fn upgrade_authority_is_read_from_the_program_data_account() {
        let loader = bpf_loader_upgradeable::id();
        let program = vec![0u8; 36];
        let authority = Pubkey::new_unique();
        let mut with_authority = vec![0u8; 45];
        with_authority[12] = 1;
        with_authority[13..45].copy_from_slice(authority.as_ref());
        assert_eq!(
            upgrade_info(loader, &program, Some(&with_authority)),
            UpgradeInfo::Authority(authority)
        );
        let mut revoked = vec![0u8; 45];
        revoked[12] = 0;
        assert_eq!(
            upgrade_info(loader, &program, Some(&revoked)),
            UpgradeInfo::Immutable
        );
        // A program owned by another loader is not upgradeable-loader state.
        let other = Pubkey::new_unique();
        assert_eq!(
            upgrade_info(other, &program, None),
            UpgradeInfo::NotApplicable(other)
        );
    }

    #[test]
    fn problems_name_what_stops_a_transfer() {
        let mint = Pubkey::new_unique();
        let hook = Pubkey::new_unique();
        let list = Pubkey::new_unique();
        let ready = Readiness {
            mint,
            mint_exists: true,
            token_2022: true,
            has_hook_extension: true,
            hook_program: Some(hook),
            hook_authority: None,
            program: Some(ProgramFacts {
                address: hook,
                exists: true,
                executable: true,
                loader: None,
                upgrade: None,
            }),
            validation_list: Some(ValidationListFacts {
                address: list,
                exists: true,
                owned_by_hook: true,
                has_execute_shape: true,
                extra_accounts: Some(1),
            }),
        };
        assert!(ready.is_transport_ready());

        let mut missing_list = ready.clone();
        missing_list.validation_list.as_mut().unwrap().exists = false;
        assert!(missing_list.transport_problems()[0].contains("does not exist"));
        let mut foreign_list = ready.clone();
        foreign_list.validation_list.as_mut().unwrap().owned_by_hook = false;
        assert!(foreign_list.transport_problems()[0].contains("not owned by the hook"));
        let mut dead_program = ready.clone();
        dead_program.program.as_mut().unwrap().executable = false;
        assert!(dead_program.transport_problems()[0].contains("not executable"));
        let mut no_extension = ready;
        no_extension.has_hook_extension = false;
        assert!(no_extension.transport_problems()[0].contains("no TransferHook extension"));
    }
}
