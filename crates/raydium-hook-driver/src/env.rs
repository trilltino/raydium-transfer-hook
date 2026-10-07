//! Environment manifests (`environments/*.json`). Program ids are never constants in code: every
//! flow reads them from a manifest, so the same flow runs against a local build, the integration
//! devnet deployment, or (once Raydium ships hook support) the official programs.

use std::{collections::BTreeMap, fs, path::Path, str::FromStr};

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use crate::chain::{DriverError, Result};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Programs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpmm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clmm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_hook: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arbitrary_hook: Option<String>,
}

/// One on-chain deployment, recorded as evidence.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Deployment {
    pub name: String,
    pub program_id: String,
    pub signature: String,
    pub artifact_sha256: String,
    pub artifact_bytes: u64,
    pub upgrade_authority: String,
    pub source: String,
}

/// One executed step of an end-to-end run, recorded as evidence.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evidence {
    pub flow: String,
    pub step: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub detail: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    /// `localnet`, `devnet`, ...
    pub cluster: String,
    pub rpc_url: String,
    /// `integration`: programs built from our hook-support forks and deployed under our own ids.
    /// `official`: Raydium's own deployments (must not be sent hook-aware instructions).
    pub kind: String,
    pub programs: Programs,
    /// Admin key of the integration CPMM/CLMM builds (public key only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin: Option<String>,
    /// CPMM pool-creation fee receiver: a wrapped-SOL token account at this address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpmm_fee_receiver: Option<String>,
    /// Source revisions the deployed binaries were built from.
    #[serde(default)]
    pub sources: BTreeMap<String, String>,
    #[serde(default)]
    pub deployments: Vec<Deployment>,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

fn parse(label: &str, value: &Option<String>) -> Result<Pubkey> {
    let text = value
        .as_deref()
        .ok_or_else(|| DriverError::new(format!("environment has no `{label}`")))?;
    Pubkey::from_str(text).map_err(|e| DriverError::new(format!("`{label}` is not a pubkey: {e}")))
}

impl Environment {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let text = fs::read_to_string(path.as_ref())
            .map_err(|e| DriverError::new(format!("read {}: {e}", path.as_ref().display())))?;
        serde_json::from_str(&text)
            .map_err(|e| DriverError::new(format!("parse {}: {e}", path.as_ref().display())))
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let mut text = serde_json::to_string_pretty(self)
            .map_err(|e| DriverError::new(format!("serialise environment: {e}")))?;
        text.push('\n');
        fs::write(path.as_ref(), text)
            .map_err(|e| DriverError::new(format!("write {}: {e}", path.as_ref().display())))
    }

    pub fn cpmm_program(&self) -> Result<Pubkey> {
        parse("programs.cpmm", &self.programs.cpmm)
    }

    pub fn clmm_program(&self) -> Result<Pubkey> {
        parse("programs.clmm", &self.programs.clmm)
    }

    pub fn reference_hook_program(&self) -> Result<Pubkey> {
        parse("programs.reference_hook", &self.programs.reference_hook)
    }

    pub fn arbitrary_hook_program(&self) -> Result<Pubkey> {
        parse("programs.arbitrary_hook", &self.programs.arbitrary_hook)
    }

    pub fn admin_key(&self) -> Result<Pubkey> {
        parse("admin", &self.admin)
    }

    pub fn cpmm_fee_receiver_key(&self) -> Result<Pubkey> {
        parse("cpmm_fee_receiver", &self.cpmm_fee_receiver)
    }

    /// Refuse to send hook-aware instructions to Raydium's own programs.
    pub fn require_hook_aware(&self) -> Result<()> {
        if self.kind == "official" {
            return Err(DriverError::new(
                "this environment points at Raydium's official programs, which do not contain \
                 the hook-aware instructions (swap_base_input_v2 / swap_v3)",
            ));
        }
        Ok(())
    }
}
