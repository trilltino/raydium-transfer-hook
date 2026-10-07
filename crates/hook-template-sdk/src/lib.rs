#![forbid(unsafe_code)]
//! The optional template-descriptor standard for Transfer Hooks.
//!
//! A template is a reusable commercial rule (a vesting floor, a launch window). A hook program can
//! host many. Describing one is optional metadata for people choosing hooks; **it is never
//! permission, and nothing in Raydium, the SDK or this repository requires it.**
//!
//! | Module | Role |
//! |---|---|
//! | [`manifest`] | the off-chain manifest, its canonical form, and the content-derived id |
//! | [`trust`] | what a reader may conclude from a descriptor (and what they may not) |
//!
//! The on-chain side is `hook-template-registry`: [`descriptor_address`], [`Descriptor`] and the
//! `publish`, `update` and `close` instruction builders are re-exported here.

pub mod manifest;
pub mod trust;

pub use hook_template_registry::{
    descriptor::{
        descriptor_address, Descriptor, DESCRIPTOR_DISCRIMINATOR, DESCRIPTOR_LEN,
        DESCRIPTOR_VERSION,
    },
    error::RegistryError,
    instruction::{close, publish, update},
};
pub use manifest::{canonical_json, manifest_hash, template_id, ManifestError};
pub use trust::{assess, Assessment, TrustFacts};
