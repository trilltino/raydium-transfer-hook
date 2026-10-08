//! Structured, assertable errors for resolution and framing.
//!
//! Every error type is `Clone + PartialEq + Eq` and `#[non_exhaustive]` where
//! new variants are expected, so callers can both match on them in tests and
//! keep compiling when variants are added.

mod fetch;
mod frame;
mod leg;
mod resolve;

pub use fetch::FetchError;
pub use frame::{ConflictSite, FrameError, SliceFault};
pub use leg::{LegError, LegField, LegRole};
pub use resolve::{
    AuthorityExpectation, HookChangeKind, HookProgramInvalidReason, SplResolveError,
};
