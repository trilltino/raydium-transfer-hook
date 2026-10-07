//! The generic external-hook provider: a hook the stack knows **only by program id**, whose setup
//! is described as data.
//!
//! Token-2022 defines how a hook executes, not how it is initialised, so every hook has its own
//! setup instructions. Instead of writing Rust for each, a hook author (or an integrator who did
//! not write the hook) describes the setup in JSON, and this provider turns it into instructions.
//! Nothing here depends on the hook's own crate.
//!
//! ```json
//! {
//!   "program_id": "EtXNoNoYQdF9QFaSttjGzZkAk29EcBW9apdcE4En6WLB",
//!   "setup": [{
//!     "accounts": [
//!       { "key": "{payer}", "signer": true, "writable": true },
//!       { "key": "{hooked_mint}" },
//!       { "key": "{payer}", "signer": true },
//!       { "pda": { "program": "{program}", "seeds": ["utf8:arb-policy", "key:{hooked_mint}"] }, "writable": true }
//!     ],
//!     "data_hex": "4152424e495431" "02000000"
//!   }],
//!   "allowed_writable": [
//!     { "pda": { "program": "{program}", "seeds": ["utf8:arb-stats", "key:{hooked_mint}"] } }
//!   ],
//!   "state_account": { "pda": { "program": "{program}", "seeds": ["utf8:arb-stats", "key:{hooked_mint}"] } },
//!   "refusals": [
//!     { "direction": "hooked_in", "plan": { "repeat": { "times": 3 } }, "code": 36865 }
//!   ]
//! }
//! ```
//!
//! # Placeholders
//!
//! Anywhere a key is expected (an account, a PDA program, a `key:` seed) `{name}` stands for an
//! address that only exists once a flow has run: `{program}`, `{payer}`, `{hooked_mint}`,
//! `{quote_mint}`, `{trader_hooked}`, `{trader_quote}`, `{vault_hooked}`, `{vault_quote}`,
//! `{pool_authority}`, `{system_program}`, `{token_2022}`. "Hooked" is the mint this hook is on.
//! `allowed_writable` can also use `{leg_source}` and `{leg_destination}`, the two accounts of the
//! transfer being resolved. `state_account` may only use `{program}` and `{hooked_mint}`.
//!
//! Seeds are `utf8:<text>`, `hex:<bytes>` or `key:<address or placeholder>`.

use std::{collections::HashMap, str::FromStr};

use serde::Deserialize;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use transfer_hook_sdk::SplTransferLeg;

use super::{point_mint_at_hook, Direction, HookContext, HookSetup, Refusal, RejectionPlan};
use crate::chain::{DriverError, Result};

/// A key: a literal address, a `{placeholder}`, or a PDA.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum KeyRef {
    Pda { pda: PdaSpec },
    Key { key: String },
}

#[derive(Clone, Debug, Deserialize)]
pub struct PdaSpec {
    pub program: String,
    pub seeds: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccountSpec {
    #[serde(flatten)]
    pub key: KeyRef,
    #[serde(default)]
    pub signer: bool,
    #[serde(default)]
    pub writable: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct InstructionSpec {
    /// Defaults to the hook program.
    #[serde(default)]
    pub program: Option<String>,
    pub accounts: Vec<AccountSpec>,
    /// The instruction data as hex (no `0x`).
    #[serde(default)]
    pub data_hex: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectionSpec {
    HookedIn,
    HookedOut,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanSpec {
    OverAmount { amount_in: u64 },
    Repeat { times: usize },
    PriorityFee { amount_in: u64, micro_lamports: u64 },
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct RefusalSpec {
    pub direction: DirectionSpec,
    pub plan: PlanSpec,
    pub code: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GenericHookSpec {
    pub program_id: String,
    /// Instructions to send after the mint has been pointed at the hook, to initialise it.
    #[serde(default)]
    pub setup: Vec<InstructionSpec>,
    /// Writable extra accounts the integrator accepts (the SDK refuses any other writable extra).
    #[serde(default)]
    pub allowed_writable: Vec<KeyRef>,
    /// An account the hook writes on every allowed transfer; the flow checks it changes.
    #[serde(default)]
    pub state_account: Option<KeyRef>,
    /// Swaps the hook must refuse, and with which error code.
    #[serde(default)]
    pub refusals: Vec<RefusalSpec>,
}

/// The addresses a placeholder can stand for.
#[derive(Default)]
struct Vars(HashMap<&'static str, Pubkey>);

impl Vars {
    fn from_context(program: Pubkey, ctx: &HookContext) -> Self {
        let mut vars = HashMap::new();
        vars.insert("program", program);
        vars.insert("payer", ctx.payer);
        vars.insert("hooked_mint", ctx.hooked_mint);
        vars.insert("quote_mint", ctx.quote_mint);
        vars.insert("trader_hooked", ctx.trader_accounts[0]);
        vars.insert("trader_quote", ctx.trader_accounts[1]);
        vars.insert("vault_hooked", ctx.vaults[0]);
        vars.insert("vault_quote", ctx.vaults[1]);
        vars.insert("pool_authority", ctx.pool_authority);
        vars.insert("system_program", solana_sdk::system_program::id());
        vars.insert("token_2022", spl_token_2022::id());
        Self(vars)
    }

    fn with(mut self, name: &'static str, key: Pubkey) -> Self {
        self.0.insert(name, key);
        self
    }

    fn key(&self, text: &str) -> Result<Pubkey> {
        if let Some(name) = text.strip_prefix('{').and_then(|t| t.strip_suffix('}')) {
            return self.0.get(name).copied().ok_or_else(|| {
                DriverError::new(format!("unknown or unavailable placeholder `{text}`"))
            });
        }
        Pubkey::from_str(text).map_err(|e| {
            DriverError::new(format!(
                "`{text}` is neither a placeholder nor an address: {e}"
            ))
        })
    }

    fn resolve(&self, key: &KeyRef) -> Result<Pubkey> {
        match key {
            KeyRef::Key { key } => self.key(key),
            KeyRef::Pda { pda } => {
                let program = self.key(&pda.program)?;
                let seeds = pda
                    .seeds
                    .iter()
                    .map(|seed| self.seed(seed))
                    .collect::<Result<Vec<_>>>()?;
                let refs: Vec<&[u8]> = seeds.iter().map(Vec::as_slice).collect();
                Ok(Pubkey::find_program_address(&refs, &program).0)
            }
        }
    }

    fn seed(&self, text: &str) -> Result<Vec<u8>> {
        if let Some(rest) = text.strip_prefix("utf8:") {
            Ok(rest.as_bytes().to_vec())
        } else if let Some(rest) = text.strip_prefix("hex:") {
            hex_decode(rest)
        } else if let Some(rest) = text.strip_prefix("key:") {
            Ok(self.key(rest)?.to_bytes().to_vec())
        } else {
            Err(DriverError::new(format!(
                "seed `{text}` must start with utf8:, hex: or key:"
            )))
        }
    }

    fn instruction(&self, spec: &InstructionSpec) -> Result<Instruction> {
        let program = match &spec.program {
            Some(program) => self.key(program)?,
            None => self.key("{program}")?,
        };
        let accounts = spec
            .accounts
            .iter()
            .map(|account| {
                let key = self.resolve(&account.key)?;
                Ok(match (account.signer, account.writable) {
                    (true, true) => AccountMeta::new(key, true),
                    (true, false) => AccountMeta::new_readonly(key, true),
                    (false, true) => AccountMeta::new(key, false),
                    (false, false) => AccountMeta::new_readonly(key, false),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Instruction {
            program_id: program,
            accounts,
            data: hex_decode(&spec.data_hex)?,
        })
    }
}

fn hex_decode(text: &str) -> Result<Vec<u8>> {
    let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if text.len() % 2 != 0 {
        return Err(DriverError::new(format!("hex `{text}` has an odd length")));
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16)
                .map_err(|_| DriverError::new(format!("`{}` is not hex", &text[i..i + 2])))
        })
        .collect()
}

/// A hook described entirely as data. See the module documentation.
pub struct GenericExternalHook {
    spec: GenericHookSpec,
    program: Pubkey,
}

impl GenericExternalHook {
    /// Parse a JSON description and check it end to end: every placeholder must exist, every key
    /// and hex string must parse. A description that passes cannot fail later.
    pub fn from_json(json: &str) -> Result<Self> {
        let spec: GenericHookSpec = serde_json::from_str(json)
            .map_err(|e| DriverError::new(format!("the hook description is not valid: {e}")))?;
        let program = Pubkey::from_str(&spec.program_id)
            .map_err(|e| DriverError::new(format!("`program_id` is not an address: {e}")))?;
        let hook = Self { spec, program };
        hook.validate()?;
        Ok(hook)
    }

    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path = path.as_ref();
        let json = std::fs::read_to_string(path)
            .map_err(|e| DriverError::new(format!("read {}: {e}", path.display())))?;
        Self::from_json(&json)
    }

    /// Resolve everything once against a dummy context, so mistakes surface at load time.
    fn validate(&self) -> Result<()> {
        let dummy = HookContext {
            payer: Pubkey::new_unique(),
            hooked_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            trader_accounts: [Pubkey::new_unique(), Pubkey::new_unique()],
            pool_authority: Pubkey::new_unique(),
            vaults: [Pubkey::new_unique(), Pubkey::new_unique()],
            now: 0,
        };
        let vars = Vars::from_context(self.program, &dummy);
        for instruction in &self.spec.setup {
            vars.instruction(instruction)?;
        }
        let leg_vars = Vars::from_context(self.program, &dummy)
            .with("leg_source", Pubkey::new_unique())
            .with("leg_destination", Pubkey::new_unique());
        for key in &self.spec.allowed_writable {
            leg_vars.resolve(key)?;
        }
        if let Some(state) = &self.spec.state_account {
            self.state_vars(&dummy.hooked_mint).resolve(state)?;
        }
        Ok(())
    }

    fn state_vars(&self, mint: &Pubkey) -> Vars {
        let mut vars = Vars::default();
        vars.0.insert("program", self.program);
        vars.0.insert("hooked_mint", *mint);
        vars
    }
}

impl HookSetup for GenericExternalHook {
    fn name(&self) -> &'static str {
        "external hook (generic setup)"
    }

    fn program_id(&self) -> Pubkey {
        self.program
    }

    fn enable_instructions(&self, ctx: &HookContext) -> Vec<Instruction> {
        let vars = Vars::from_context(self.program, ctx);
        let mut instructions = vec![point_mint_at_hook(
            &ctx.hooked_mint,
            &ctx.payer,
            &self.program,
        )];
        instructions.extend(
            self.spec
                .setup
                .iter()
                .map(|spec| vars.instruction(spec).expect("validated when loaded")),
        );
        instructions
    }

    fn refusals(&self) -> Vec<Refusal> {
        self.spec
            .refusals
            .iter()
            .map(|r| Refusal {
                direction: match r.direction {
                    DirectionSpec::HookedIn => Direction::HookedIn,
                    DirectionSpec::HookedOut => Direction::HookedOut,
                },
                plan: match r.plan {
                    PlanSpec::OverAmount { amount_in } => RejectionPlan::OverAmount { amount_in },
                    PlanSpec::Repeat { times } => RejectionPlan::RepeatInOneTransaction { times },
                    PlanSpec::PriorityFee {
                        amount_in,
                        micro_lamports,
                    } => RejectionPlan::HighPriorityFee {
                        amount_in,
                        micro_lamports,
                    },
                },
                code: r.code,
            })
            .collect()
    }

    fn state_account(&self, mint: &Pubkey) -> Option<Pubkey> {
        self.spec.state_account.as_ref().map(|state| {
            self.state_vars(mint)
                .resolve(state)
                .expect("validated when loaded")
        })
    }

    fn allowed_writable(&self, ctx: &HookContext, leg: &SplTransferLeg) -> Vec<Pubkey> {
        let vars = Vars::from_context(self.program, ctx)
            .with("leg_source", leg.source)
            .with("leg_destination", leg.destination);
        self.spec
            .allowed_writable
            .iter()
            .map(|key| vars.resolve(key).expect("validated when loaded"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program() -> Pubkey {
        Pubkey::new_unique()
    }

    fn json(program: &Pubkey, extra: &str) -> String {
        format!(r#"{{"program_id": "{program}"{extra}}}"#)
    }

    #[test]
    fn a_description_resolves_placeholders_and_pdas_for_each_mint() {
        let program = program();
        let hook = GenericExternalHook::from_json(&json(
            &program,
            r#", "setup": [{
                "accounts": [
                    {"key": "{payer}", "signer": true, "writable": true},
                    {"key": "{hooked_mint}"},
                    {"pda": {"program": "{program}", "seeds": ["utf8:cfg", "key:{hooked_mint}"]}, "writable": true}
                ],
                "data_hex": "01 02 ff"
            }],
            "state_account": {"pda": {"program": "{program}", "seeds": ["utf8:cfg", "key:{hooked_mint}"]}}"#,
        ))
        .unwrap();
        let ctx = HookContext {
            payer: Pubkey::new_unique(),
            hooked_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            trader_accounts: [Pubkey::new_unique(); 2],
            pool_authority: Pubkey::new_unique(),
            vaults: [Pubkey::new_unique(); 2],
            now: 0,
        };
        let instructions = hook.enable_instructions(&ctx);
        assert_eq!(
            instructions.len(),
            2,
            "pointing the mint at the hook, then the setup"
        );
        let setup = &instructions[1];
        assert_eq!(setup.program_id, program);
        assert_eq!(setup.data, vec![1, 2, 255]);
        assert!(setup.accounts[0].is_signer && setup.accounts[0].is_writable);
        let (expected, _) =
            Pubkey::find_program_address(&[b"cfg", ctx.hooked_mint.as_ref()], &program);
        assert_eq!(setup.accounts[2].pubkey, expected);
        // The state account is the same PDA, derived from the mint alone.
        assert_eq!(hook.state_account(&ctx.hooked_mint), Some(expected));
    }

    #[test]
    fn refusals_and_writable_extras_follow_the_description() {
        let program = program();
        let hook = GenericExternalHook::from_json(&json(
            &program,
            r#", "allowed_writable": [{"key": "{leg_source}"}, {"key": "{trader_quote}"}],
            "refusals": [
                {"direction": "hooked_in", "plan": {"repeat": {"times": 3}}, "code": 4660},
                {"direction": "hooked_out", "plan": {"over_amount": {"amount_in": 600}}, "code": 1}
            ]"#,
        ))
        .unwrap();
        let refusals = hook.refusals();
        assert_eq!(refusals.len(), 2);
        assert_eq!(refusals[0].code, 4660);
        assert_eq!(
            refusals[0].plan,
            RejectionPlan::RepeatInOneTransaction { times: 3 }
        );
        assert_eq!(refusals[1].direction, Direction::HookedOut);
    }

    #[test]
    fn mistakes_are_reported_when_the_description_is_loaded() {
        let program = program();
        let load = |extra: &str| GenericExternalHook::from_json(&json(&program, extra));
        assert!(load(r#", "setup": [{"accounts": [{"key": "{nope}"}]}]"#).is_err());
        assert!(load(r#", "setup": [{"accounts": [], "data_hex": "abc"}]"#).is_err());
        assert!(load(r#", "setup": [{"accounts": [], "data_hex": "zz"}]"#).is_err());
        assert!(load(
            r#", "setup": [{"accounts": [{"pda": {"program": "{program}", "seeds": ["bad"]}}]}]"#
        )
        .is_err());
        // The state account only knows the hook program and its own mint.
        assert!(load(r#", "state_account": {"key": "{payer}"}"#).is_err());
        assert!(GenericExternalHook::from_json("{}").is_err());
        assert!(GenericExternalHook::from_json(r#"{"program_id": "not a key"}"#).is_err());
    }
}
