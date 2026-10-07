//! The registry's instructions.
//!
//! | Instruction | Who | What |
//! |---|---|---|
//! | `Publish` | anyone | create their own descriptor for `(hook program, template id)` |
//! | `Update` | the descriptor's authority | change the manifest hash and flags |
//! | `Close` | the descriptor's authority | delete the descriptor and take the rent back |
//!
//! There is no instruction that allows, approves or ranks a hook, and none that only a registry
//! owner can call: publishing is permissionless and means nothing about the hook's trustworthiness.

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

use crate::{descriptor::descriptor_address, error::RegistryError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistryInstruction {
    Publish {
        template_id: [u8; 32],
        manifest_hash: [u8; 32],
        flags: u64,
    },
    Update {
        manifest_hash: [u8; 32],
        flags: u64,
    },
    Close,
}

impl RegistryInstruction {
    pub fn unpack(data: &[u8]) -> Result<Self, RegistryError> {
        let bad = RegistryError::InvalidInstruction;
        match data.split_first() {
            Some((0, rest)) if rest.len() == 32 + 32 + 8 => Ok(Self::Publish {
                template_id: rest[..32].try_into().map_err(|_| bad)?,
                manifest_hash: rest[32..64].try_into().map_err(|_| bad)?,
                flags: u64::from_le_bytes(rest[64..72].try_into().map_err(|_| bad)?),
            }),
            Some((1, rest)) if rest.len() == 32 + 8 => Ok(Self::Update {
                manifest_hash: rest[..32].try_into().map_err(|_| bad)?,
                flags: u64::from_le_bytes(rest[32..40].try_into().map_err(|_| bad)?),
            }),
            Some((2, [])) => Ok(Self::Close),
            _ => Err(bad),
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        match self {
            Self::Publish {
                template_id,
                manifest_hash,
                flags,
            } => {
                let mut data = vec![0];
                data.extend_from_slice(template_id);
                data.extend_from_slice(manifest_hash);
                data.extend_from_slice(&flags.to_le_bytes());
                data
            }
            Self::Update {
                manifest_hash,
                flags,
            } => {
                let mut data = vec![1];
                data.extend_from_slice(manifest_hash);
                data.extend_from_slice(&flags.to_le_bytes());
                data
            }
            Self::Close => vec![2],
        }
    }
}

/// Build `Publish`: `publisher` signs and pays, and becomes the descriptor's authority.
pub fn publish(
    registry_program: &Pubkey,
    publisher: &Pubkey,
    hook_program: &Pubkey,
    template_id: [u8; 32],
    manifest_hash: [u8; 32],
    flags: u64,
) -> Instruction {
    Instruction {
        program_id: *registry_program,
        accounts: vec![
            AccountMeta::new(*publisher, true),
            AccountMeta::new_readonly(*hook_program, false),
            AccountMeta::new(
                descriptor_address(registry_program, hook_program, &template_id, publisher).0,
                false,
            ),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: RegistryInstruction::Publish {
            template_id,
            manifest_hash,
            flags,
        }
        .pack(),
    }
}

/// Build `Update` for the descriptor at `descriptor`.
pub fn update(
    registry_program: &Pubkey,
    authority: &Pubkey,
    descriptor: &Pubkey,
    manifest_hash: [u8; 32],
    flags: u64,
) -> Instruction {
    Instruction {
        program_id: *registry_program,
        accounts: vec![
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*descriptor, false),
        ],
        data: RegistryInstruction::Update {
            manifest_hash,
            flags,
        }
        .pack(),
    }
}

/// Build `Close` for the descriptor at `descriptor`; the rent goes to `authority`.
pub fn close(registry_program: &Pubkey, authority: &Pubkey, descriptor: &Pubkey) -> Instruction {
    Instruction {
        program_id: *registry_program,
        accounts: vec![
            AccountMeta::new(*authority, true),
            AccountMeta::new(*descriptor, false),
        ],
        data: RegistryInstruction::Close.pack(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_round_trip() {
        for instruction in [
            RegistryInstruction::Publish {
                template_id: [1; 32],
                manifest_hash: [2; 32],
                flags: u64::MAX,
            },
            RegistryInstruction::Update {
                manifest_hash: [3; 32],
                flags: 4,
            },
            RegistryInstruction::Close,
        ] {
            assert_eq!(
                RegistryInstruction::unpack(&instruction.pack()),
                Ok(instruction)
            );
        }
        assert!(RegistryInstruction::unpack(&[]).is_err());
        assert!(RegistryInstruction::unpack(&[0, 1, 2]).is_err());
        assert!(RegistryInstruction::unpack(&[9]).is_err());
    }
}
