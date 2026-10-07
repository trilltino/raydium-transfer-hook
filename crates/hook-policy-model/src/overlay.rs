//! The platform hook settings as bytes laid over a reserved region of a platform account, and the
//! rule that locks the hook program after the first hooked mint exists.
//!
//! **MODEL ONLY.** The layout below is a design: it assumes an existing platform account has a
//! 180-byte region of zeroed padding that can be overlaid without resizing the account (the
//! reviewed public CPI state of LaunchLab's `PlatformConfig` exposes 180 bytes of padding). Nothing
//! here has been checked against LaunchLab's deployed program, whose handler is not public, and no
//! on-chain account is read or written. Before storing this anywhere, inspect the exact
//! serialization, padding and upgrade compatibility; do not assume unused bytes are safe to reuse.
//!
//! ```text
//! offset  size  field
//!      0    32  hook program           (all zero: none)
//!     32     1  hook policy            0 Disabled, 1 Optional, 2 Mandatory
//!     33     1  hook authority policy  0 PlatformRetained, 1 ImmutableAtLaunch, 2 GovernedTimelock
//!     34     1  hook config version
//!     35     1  hook flags             bit 0: program identity locked
//!     36   144  reserved               preserved verbatim, never interpreted
//! ```
//!
//! An all-zero region decodes as "no hook": no program, `Disabled`, version 0, no flags. So an
//! existing platform account whose padding was never written keeps meaning exactly what it meant.

use crate::{
    error::PolicyError,
    platform::PlatformConfig,
    policy::{HookAuthorityPolicy, HookPolicy},
    types::Pubkey,
};

/// Bytes of the overlay.
pub const OVERLAY_LEN: usize = 180;
/// Reserved bytes at the end, kept for later fields.
pub const RESERVED_HOOK_BYTES: usize = 144;

/// Flag bit: the hook program can no longer change.
pub const FLAG_IDENTITY_LOCKED: u8 = 1;

/// The platform's hook settings as stored in the reserved region.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformHookOverlay {
    /// The hook program; all zero means none.
    pub program: Pubkey,
    pub policy: HookPolicy,
    pub authority_policy: HookAuthorityPolicy,
    pub version: u8,
    pub flags: u8,
    pub reserved: [u8; RESERVED_HOOK_BYTES],
}

fn policy_byte(policy: HookPolicy) -> u8 {
    match policy {
        HookPolicy::Disabled => 0,
        HookPolicy::Optional => 1,
        HookPolicy::Mandatory => 2,
    }
}

fn authority_byte(policy: HookAuthorityPolicy) -> u8 {
    match policy {
        HookAuthorityPolicy::PlatformRetained => 0,
        HookAuthorityPolicy::ImmutableAtLaunch => 1,
        HookAuthorityPolicy::GovernedTimelock => 2,
    }
}

impl PlatformHookOverlay {
    /// What an untouched (all-zero) region means: no hook.
    pub fn none() -> Self {
        Self {
            program: [0; 32],
            policy: HookPolicy::Disabled,
            authority_policy: HookAuthorityPolicy::PlatformRetained,
            version: 0,
            flags: 0,
            reserved: [0; RESERVED_HOOK_BYTES],
        }
    }

    pub fn decode(bytes: &[u8; OVERLAY_LEN]) -> Result<Self, PolicyError> {
        let policy = match bytes[32] {
            0 => HookPolicy::Disabled,
            1 => HookPolicy::Optional,
            2 => HookPolicy::Mandatory,
            _ => return Err(PolicyError::UnknownPolicyByte),
        };
        let authority_policy = match bytes[33] {
            0 => HookAuthorityPolicy::PlatformRetained,
            1 => HookAuthorityPolicy::ImmutableAtLaunch,
            2 => HookAuthorityPolicy::GovernedTimelock,
            _ => return Err(PolicyError::UnknownPolicyByte),
        };
        let mut program = [0; 32];
        program.copy_from_slice(&bytes[..32]);
        let mut reserved = [0; RESERVED_HOOK_BYTES];
        reserved.copy_from_slice(&bytes[36..]);
        Ok(Self {
            program,
            policy,
            authority_policy,
            version: bytes[34],
            flags: bytes[35],
            reserved,
        })
    }

    pub fn encode(&self) -> [u8; OVERLAY_LEN] {
        let mut bytes = [0; OVERLAY_LEN];
        bytes[..32].copy_from_slice(&self.program);
        bytes[32] = policy_byte(self.policy);
        bytes[33] = authority_byte(self.authority_policy);
        bytes[34] = self.version;
        bytes[35] = self.flags;
        bytes[36..].copy_from_slice(&self.reserved);
        bytes
    }

    /// The hook program, or `None` for the all-zero key.
    pub fn hook_program(&self) -> Option<Pubkey> {
        (self.program != [0; 32]).then_some(self.program)
    }

    pub fn is_identity_locked(&self) -> bool {
        self.flags & FLAG_IDENTITY_LOCKED != 0
    }

    /// The settings as the policy model's [`PlatformConfig`].
    pub fn platform_config(&self) -> PlatformConfig {
        PlatformConfig::new(self.hook_program(), self.policy, self.authority_policy)
    }

    /// Record that a hooked mint was created: from now on the program identity is locked.
    ///
    /// A platform that wants a different hook engine afterwards creates another platform account.
    /// (The program itself can still host many rule templates and many per-mint configurations.)
    pub fn record_hooked_mint_created(&mut self) {
        self.flags |= FLAG_IDENTITY_LOCKED;
    }

    /// Whether the hook program may be set to `new` (`None` clears it). Before the lock anything
    /// goes (subject to the authority policy, checked separately); after it, only the same program.
    pub fn check_program_change(&self, new: Option<Pubkey>) -> Result<(), PolicyError> {
        if self.is_identity_locked() && new != self.hook_program() {
            return Err(PolicyError::HookIdentityLocked);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> Pubkey {
        [byte; 32]
    }

    #[test]
    fn an_all_zero_region_means_no_hook() {
        let overlay = PlatformHookOverlay::decode(&[0; OVERLAY_LEN]).unwrap();
        assert_eq!(overlay, PlatformHookOverlay::none());
        assert_eq!(overlay.hook_program(), None);
        assert_eq!(overlay.policy, HookPolicy::Disabled);
        assert!(!overlay.is_identity_locked());
        // And so existing platform accounts keep their meaning.
        assert_eq!(overlay.encode(), [0; OVERLAY_LEN]);
    }

    #[test]
    fn the_layout_is_pinned_byte_for_byte() {
        let mut reserved = [0; RESERVED_HOOK_BYTES];
        reserved[0] = 0xAA;
        reserved[RESERVED_HOOK_BYTES - 1] = 0xBB;
        let overlay = PlatformHookOverlay {
            program: key(7),
            policy: HookPolicy::Mandatory,
            authority_policy: HookAuthorityPolicy::GovernedTimelock,
            version: 3,
            flags: FLAG_IDENTITY_LOCKED,
            reserved,
        };
        let bytes = overlay.encode();
        assert_eq!(bytes.len(), 180);
        assert_eq!(&bytes[..32], &[7; 32]);
        assert_eq!(bytes[32], 2);
        assert_eq!(bytes[33], 2);
        assert_eq!(bytes[34], 3);
        assert_eq!(bytes[35], 1);
        assert_eq!(bytes[36], 0xAA);
        assert_eq!(bytes[179], 0xBB);
        assert_eq!(PlatformHookOverlay::decode(&bytes), Ok(overlay));
    }

    #[test]
    fn reserved_bytes_survive_a_round_trip_untouched() {
        let mut bytes = [0u8; OVERLAY_LEN];
        for (i, byte) in bytes[36..].iter_mut().enumerate() {
            *byte = i as u8 + 1;
        }
        let decoded = PlatformHookOverlay::decode(&bytes).unwrap();
        assert_eq!(decoded.encode(), bytes);
    }

    #[test]
    fn unknown_policy_bytes_are_rejected_not_guessed() {
        let mut bytes = [0u8; OVERLAY_LEN];
        bytes[32] = 3;
        assert_eq!(
            PlatformHookOverlay::decode(&bytes),
            Err(PolicyError::UnknownPolicyByte)
        );
        bytes[32] = 0;
        bytes[33] = 9;
        assert_eq!(
            PlatformHookOverlay::decode(&bytes),
            Err(PolicyError::UnknownPolicyByte)
        );
    }

    #[test]
    fn the_overlay_feeds_the_platform_policy_model() {
        let overlay = PlatformHookOverlay {
            program: key(5),
            policy: HookPolicy::Optional,
            authority_policy: HookAuthorityPolicy::ImmutableAtLaunch,
            ..PlatformHookOverlay::none()
        };
        let config = overlay.platform_config();
        assert_eq!(config.hook_program, Some(key(5)));
        assert_eq!(config.policy, HookPolicy::Optional);
        assert_eq!(
            PlatformHookOverlay::none().platform_config().hook_program,
            None
        );
    }

    #[test]
    fn the_program_locks_after_the_first_hooked_mint() {
        let mut overlay = PlatformHookOverlay {
            program: key(1),
            policy: HookPolicy::Mandatory,
            ..PlatformHookOverlay::none()
        };
        // Before any hooked mint exists the program can still change or be cleared.
        assert_eq!(overlay.check_program_change(Some(key(2))), Ok(()));
        assert_eq!(overlay.check_program_change(None), Ok(()));

        overlay.record_hooked_mint_created();
        assert!(overlay.is_identity_locked());
        // After: the same program is fine, a different one or clearing it is refused.
        assert_eq!(overlay.check_program_change(Some(key(1))), Ok(()));
        assert_eq!(
            overlay.check_program_change(Some(key(2))),
            Err(PolicyError::HookIdentityLocked)
        );
        assert_eq!(
            overlay.check_program_change(None),
            Err(PolicyError::HookIdentityLocked)
        );
        // The lock survives encoding.
        let again = PlatformHookOverlay::decode(&overlay.encode()).unwrap();
        assert!(again.is_identity_locked());
    }
}
