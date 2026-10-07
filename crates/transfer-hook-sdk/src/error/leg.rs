//! Which leg of a swap an error belongs to.

use std::fmt;

use solana_program::pubkey::Pubkey;

use super::resolve::SplResolveError;

/// Which transfer of a multi-transfer instruction a hook slice belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegRole {
    Input,
    Output,
    Token0,
    Token1,
    Base,
    Quote,
    Other(u8),
}

impl fmt::Display for LegRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input => f.write_str("input"),
            Self::Output => f.write_str("output"),
            Self::Token0 => f.write_str("token_0"),
            Self::Token1 => f.write_str("token_1"),
            Self::Base => f.write_str("base"),
            Self::Quote => f.write_str("quote"),
            Self::Other(index) => write!(f, "leg#{index}"),
        }
    }
}

/// A resolution failure attributed to one leg of a multi-transfer instruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegError {
    pub leg: LegRole,
    pub mint: Pubkey,
    pub source: SplResolveError,
}

impl fmt::Display for LegError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} leg (mint {}) failed to resolve: {}",
            self.leg, self.mint, self.source
        )
    }
}

impl std::error::Error for LegError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Which field of a transfer leg disagreed with the swap instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegField {
    Mint,
    Source,
    Destination,
    Authority,
}
