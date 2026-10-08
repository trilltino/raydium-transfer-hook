//! What a flow leaves behind so a person can keep working with the pool it created: the mints, the
//! trader's token accounts and which hook is on which mint. It holds **no secrets**: the trader
//! accounts are owned by the payer, so only the payer's own keypair is needed to trade.

use std::{fs, path::Path, str::FromStr};

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use crate::chain::{DriverError, Result};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionHook {
    pub mint: String,
    pub program: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    /// `cpmm` or `clmm`.
    pub amm: String,
    pub mint_0: String,
    pub mint_1: String,
    /// The payer's token accounts of `mint_0` and `mint_1`.
    pub accounts: [String; 2],
    /// The hook on each hooked mint.
    pub hooks: Vec<SessionHook>,
    /// Writable extra accounts the hooks declare, which a swap must name to be accepted.
    pub allowed_writable: Vec<String>,
    /// The pool the flow created (CPMM: the pool-state account), when the flow recorded it.
    #[serde(default)]
    pub pool: Option<String>,
}

fn key(label: &str, text: &str) -> Result<Pubkey> {
    Pubkey::from_str(text).map_err(|e| DriverError::new(format!("session `{label}`: {e}")))
}

impl Session {
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let mut text = serde_json::to_string_pretty(self)
            .map_err(|e| DriverError::new(format!("serialise session: {e}")))?;
        text.push('\n');
        if let Some(parent) = path.as_ref().parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(path.as_ref(), text)
            .map_err(|e| DriverError::new(format!("write {}: {e}", path.as_ref().display())))
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let text = fs::read_to_string(path.as_ref())
            .map_err(|e| DriverError::new(format!("read {}: {e}", path.as_ref().display())))?;
        serde_json::from_str(&text)
            .map_err(|e| DriverError::new(format!("parse {}: {e}", path.as_ref().display())))
    }

    pub fn mint_0(&self) -> Result<Pubkey> {
        key("mint_0", &self.mint_0)
    }

    pub fn mint_1(&self) -> Result<Pubkey> {
        key("mint_1", &self.mint_1)
    }

    pub fn accounts(&self) -> Result<[Pubkey; 2]> {
        Ok([
            key("accounts[0]", &self.accounts[0])?,
            key("accounts[1]", &self.accounts[1])?,
        ])
    }

    /// `(mint, hook program)` pairs.
    pub fn hooks(&self) -> Result<Vec<(Pubkey, Pubkey)>> {
        self.hooks
            .iter()
            .map(|h| Ok((key("hook mint", &h.mint)?, key("hook program", &h.program)?)))
            .collect()
    }

    pub fn allowed_writable(&self) -> Result<Vec<Pubkey>> {
        self.allowed_writable
            .iter()
            .map(|k| key("allowed_writable", k))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_round_trips_through_a_file() {
        let (m0, m1, a0, a1, hook, extra) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let session = Session {
            amm: "cpmm".into(),
            mint_0: m0.to_string(),
            mint_1: m1.to_string(),
            accounts: [a0.to_string(), a1.to_string()],
            hooks: vec![SessionHook {
                mint: m0.to_string(),
                program: hook.to_string(),
            }],
            allowed_writable: vec![extra.to_string()],
            pool: None,
        };
        let path = std::env::temp_dir().join(format!("session-{m0}.json"));
        session.save(&path).unwrap();
        let loaded = Session::load(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.mint_0().unwrap(), m0);
        assert_eq!(loaded.accounts().unwrap(), [a0, a1]);
        assert_eq!(loaded.hooks().unwrap(), vec![(m0, hook)]);
        assert_eq!(loaded.allowed_writable().unwrap(), vec![extra]);
    }

    #[test]
    fn a_bad_key_names_the_field() {
        let mut session = Session {
            amm: "clmm".into(),
            mint_0: "nope".into(),
            mint_1: Pubkey::new_unique().to_string(),
            accounts: [
                Pubkey::new_unique().to_string(),
                Pubkey::new_unique().to_string(),
            ],
            hooks: vec![],
            allowed_writable: vec![],
            pool: None,
        };
        assert!(session.mint_0().unwrap_err().message.contains("mint_0"));
        session.mint_0 = Pubkey::new_unique().to_string();
        assert!(session.mint_0().is_ok());
    }
}
