//! The descriptor account: who published what about which hook.
//!
//! Layout (little-endian, 209 bytes): `b"HKTMPL01"`, `bump u8`, `version u8`, `hook_program [32]`,
//! `template_id [32]`, `manifest_hash [32]`, `template_authority [32]`, `flags u64`,
//! `reserved [64]`.
//!
//! The descriptor holds hashes and keys, never prose: the manifest itself lives off-chain and
//! `manifest_hash` lets anyone check a copy they were given.

use solana_program::pubkey::Pubkey;

use crate::error::RegistryError;

pub const DESCRIPTOR_DISCRIMINATOR: [u8; 8] = *b"HKTMPL01";
pub const DESCRIPTOR_LEN: usize = 8 + 1 + 1 + 32 * 4 + 8 + 64;
pub const DESCRIPTOR_VERSION: u8 = 1;

/// The descriptor PDA: seeds `["hook-template", hook_program, template_id, publisher]`.
///
/// The publisher is part of the derivation on purpose. Publication is permissionless, so if the
/// address were only `(hook_program, template_id)` the first publisher would own it for everyone
/// and a squatter could take the address of a template they did not write. With the publisher in
/// the seeds, an account at an address was created by that publisher and nobody else, and each
/// publisher has their own descriptor. Which publisher to believe is the reader's decision (see
/// the SDK's trust assessment); the descriptor never says a hook is allowed.
pub fn descriptor_address(
    registry_program: &Pubkey,
    hook_program: &Pubkey,
    template_id: &[u8; 32],
    publisher: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            b"hook-template",
            hook_program.as_ref(),
            template_id,
            publisher.as_ref(),
        ],
        registry_program,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Descriptor {
    pub bump: u8,
    pub version: u8,
    pub hook_program: Pubkey,
    /// Content-derived: SHA-256 of the canonical template manifest.
    pub template_id: [u8; 32],
    /// SHA-256 of the manifest document as published, to check a copy of it.
    pub manifest_hash: [u8; 32],
    /// The publisher, who is the only one who can update or close the descriptor.
    pub template_authority: Pubkey,
    /// Publisher-declared. Never evidence of testing, an audit, or approval.
    pub flags: u64,
}

fn read<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], RegistryError> {
    data.get(offset..offset + N)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(RegistryError::InvalidDescriptor)
}

impl Descriptor {
    pub fn decode(data: &[u8]) -> Result<Self, RegistryError> {
        if data.len() != DESCRIPTOR_LEN || data[..8] != DESCRIPTOR_DISCRIMINATOR {
            return Err(RegistryError::InvalidDescriptor);
        }
        Ok(Self {
            bump: data[8],
            version: data[9],
            hook_program: Pubkey::new_from_array(read(data, 10)?),
            template_id: read(data, 42)?,
            manifest_hash: read(data, 74)?,
            template_authority: Pubkey::new_from_array(read(data, 106)?),
            flags: u64::from_le_bytes(read(data, 138)?),
        })
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), RegistryError> {
        if out.len() != DESCRIPTOR_LEN {
            return Err(RegistryError::InvalidDescriptor);
        }
        out.fill(0);
        out[..8].copy_from_slice(&DESCRIPTOR_DISCRIMINATOR);
        out[8] = self.bump;
        out[9] = self.version;
        out[10..42].copy_from_slice(self.hook_program.as_ref());
        out[42..74].copy_from_slice(&self.template_id);
        out[74..106].copy_from_slice(&self.manifest_hash);
        out[106..138].copy_from_slice(self.template_authority.as_ref());
        out[138..146].copy_from_slice(&self.flags.to_le_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_descriptor_round_trips_and_rejects_bad_shapes() {
        let descriptor = Descriptor {
            bump: 253,
            version: DESCRIPTOR_VERSION,
            hook_program: Pubkey::new_unique(),
            template_id: [7; 32],
            manifest_hash: [9; 32],
            template_authority: Pubkey::new_unique(),
            flags: 0xDEAD_BEEF,
        };
        let mut data = vec![0; DESCRIPTOR_LEN];
        descriptor.encode_into(&mut data).unwrap();
        assert_eq!(Descriptor::decode(&data), Ok(descriptor));
        assert!(Descriptor::decode(&data[..DESCRIPTOR_LEN - 1]).is_err());
        data[0] ^= 1;
        assert!(Descriptor::decode(&data).is_err());
    }

    #[test]
    fn the_address_depends_on_the_publisher_so_nobody_can_squat_it() {
        let registry = Pubkey::new_unique();
        let hook = Pubkey::new_unique();
        let id = [1; 32];
        let (alice, _) = descriptor_address(&registry, &hook, &id, &Pubkey::new_unique());
        let (bob, _) = descriptor_address(&registry, &hook, &id, &Pubkey::new_unique());
        assert_ne!(alice, bob);
    }
}
