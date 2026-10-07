//! What a reader may conclude from a descriptor, and what they may not.
//!
//! A descriptor is published by anyone, about any program. **Its existence never means a hook is
//! allowed**, and the flags a publisher sets on it are the publisher's own words, never evidence.
//! The levels people talk about are four different claims:
//!
//! | Claim | Can a descriptor prove it? |
//! |---|---|
//! | community-published | It is the baseline: someone put it there. |
//! | author-verified | **Yes, derivable:** the publisher is the hook program's current upgrade authority, i.e. whoever can replace the program's code. |
//! | repository-tested | No. The reader supplies whether the template's id is in a list of ids the repository tested. |
//! | audited | No. The reader supplies whether they hold an audit of that exact program and template. |
//!
//! [`assess`] only ever *derives* the second. The last two are inputs the caller vouches for from
//! sources of their own, and are passed through unchanged.

use hook_template_registry::descriptor::Descriptor;
use solana_program::pubkey::Pubkey;

/// Facts a reader brings, from sources they trust.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TrustFacts {
    /// The hook program's current upgrade authority (`None` if it is immutable, or the reader has
    /// not looked it up). `raydium_hook_driver::readiness` reports it.
    pub hook_upgrade_authority: Option<Pubkey>,
    /// The reader's own knowledge that the repository tested this template id.
    pub repository_tested: bool,
    /// The reader's own knowledge of an audit of this program and template.
    pub audited: bool,
}

/// The result of [`assess`]. `community_published` is always true.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Assessment {
    pub community_published: bool,
    pub author_verified: bool,
    pub repository_tested: bool,
    pub audited: bool,
}

impl Assessment {
    /// The strongest claim that holds, for display. A label, not a ranking of safety.
    pub fn label(&self) -> &'static str {
        if self.audited {
            "audited"
        } else if self.repository_tested {
            "repository-tested"
        } else if self.author_verified {
            "author-verified"
        } else {
            "community-published"
        }
    }
}

/// What may be concluded about `descriptor` given `facts`.
///
/// Only `author_verified` is derived (the publisher holds the hook's upgrade authority). The
/// descriptor's own flags are deliberately ignored.
pub fn assess(descriptor: &Descriptor, facts: &TrustFacts) -> Assessment {
    Assessment {
        community_published: true,
        author_verified: facts.hook_upgrade_authority == Some(descriptor.template_authority),
        repository_tested: facts.repository_tested,
        audited: facts.audited,
    }
}

#[cfg(test)]
mod tests {
    use hook_template_registry::descriptor::DESCRIPTOR_VERSION;

    use super::*;

    fn descriptor(publisher: Pubkey, flags: u64) -> Descriptor {
        Descriptor {
            bump: 255,
            version: DESCRIPTOR_VERSION,
            hook_program: Pubkey::new_unique(),
            template_id: [1; 32],
            manifest_hash: [2; 32],
            template_authority: publisher,
            flags,
        }
    }

    #[test]
    fn a_stranger_who_publishes_is_community_published_only() {
        let stranger = Pubkey::new_unique();
        let facts = TrustFacts {
            hook_upgrade_authority: Some(Pubkey::new_unique()),
            ..Default::default()
        };
        let assessment = assess(&descriptor(stranger, 0), &facts);
        assert!(assessment.community_published);
        assert!(!assessment.author_verified);
        assert_eq!(assessment.label(), "community-published");
    }

    #[test]
    fn the_publisher_who_holds_the_upgrade_authority_is_author_verified() {
        let author = Pubkey::new_unique();
        let facts = TrustFacts {
            hook_upgrade_authority: Some(author),
            ..Default::default()
        };
        let assessment = assess(&descriptor(author, 0), &facts);
        assert!(assessment.author_verified);
        assert_eq!(assessment.label(), "author-verified");
    }

    #[test]
    fn an_immutable_or_unlooked_up_hook_cannot_be_author_verified() {
        let author = Pubkey::new_unique();
        let facts = TrustFacts::default();
        assert!(!assess(&descriptor(author, 0), &facts).author_verified);
    }

    #[test]
    fn the_publishers_own_flags_prove_nothing() {
        let stranger = Pubkey::new_unique();
        let facts = TrustFacts::default();
        // Every flag bit set: still just a stranger's claim.
        let assessment = assess(&descriptor(stranger, u64::MAX), &facts);
        assert_eq!(assessment.label(), "community-published");
        assert!(!assessment.repository_tested && !assessment.audited);
    }

    #[test]
    fn tested_and_audited_are_only_what_the_reader_supplies() {
        let author = Pubkey::new_unique();
        let facts = TrustFacts {
            hook_upgrade_authority: Some(author),
            repository_tested: true,
            audited: false,
        };
        assert_eq!(
            assess(&descriptor(author, 0), &facts).label(),
            "repository-tested"
        );
        let audited = TrustFacts {
            audited: true,
            ..facts
        };
        assert_eq!(assess(&descriptor(author, 0), &audited).label(), "audited");
    }
}
