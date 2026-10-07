//! The mints and funded accounts every flow starts from.

use solana_sdk::signature::{Keypair, Signer};

use super::{recorder::Recorder, support::*};
use crate::{
    chain::{Chain, Result},
    token::{self, MintFeatures},
};

const DECIMALS: u8 = 6;
pub(super) const PROVIDER_FUNDS: u64 = 2_000_000_000;
pub(super) const TRADER_FUNDS: u64 = 10_000;

/// What the two mints carry. The first mint (`mint_0`, the smaller pubkey) always has a
/// TransferHook extension.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct WorldOptions {
    /// Also give the second mint (the quote) a TransferHook extension.
    pub(super) quote_hooked: bool,
    /// A TransferFee extension, in basis points, on both mints (0 for none).
    pub(super) transfer_fee_bps: u16,
}

impl WorldOptions {
    pub(super) fn hooked_features(self) -> MintFeatures {
        MintFeatures {
            hook: true,
            transfer_fee_bps: self.transfer_fee_bps,
        }
    }

    pub(super) fn quote_features(self) -> MintFeatures {
        MintFeatures {
            hook: self.quote_hooked,
            transfer_fee_bps: self.transfer_fee_bps,
        }
    }
}

/// The two mints (the hooked one is the smaller pubkey, hence `mint_0`) and the funded accounts.
pub(super) struct World {
    pub(super) hooked: Keypair,
    pub(super) quote: Keypair,
    pub(super) provider: [Keypair; 2],
    pub(super) trader: [Keypair; 2],
}

pub(super) async fn create_world<C: Chain>(
    chain: &mut C,
    rec: &mut Recorder,
    options: WorldOptions,
) -> Result<World> {
    let payer = chain.payer().pubkey();
    let (a, b) = (Keypair::new(), Keypair::new());
    let (hooked, quote) = if a.pubkey() < b.pubkey() {
        (a, b)
    } else {
        (b, a)
    };
    let provider = [Keypair::new(), Keypair::new()];
    let trader = [Keypair::new(), Keypair::new()];
    let (hooked_features, quote_features) = (options.hooked_features(), options.quote_features());

    send_step(
        chain,
        rec,
        &describe_mint("hooked", hooked_features),
        token::create_mint_instructions(&payer, &hooked, &payer, DECIMALS, hooked_features),
        &[&hooked],
    )
    .await?;
    send_step(
        chain,
        rec,
        &describe_mint("quote", quote_features),
        token::create_mint_instructions(&payer, &quote, &payer, DECIMALS, quote_features),
        &[&quote],
    )
    .await?;
    let mut accounts = Vec::new();
    accounts.extend(token::create_token_account_instructions(
        &payer,
        &provider[0],
        &hooked.pubkey(),
        &payer,
        hooked_features,
    ));
    accounts.extend(token::create_token_account_instructions(
        &payer,
        &provider[1],
        &quote.pubkey(),
        &payer,
        quote_features,
    ));
    send_step(
        chain,
        rec,
        "create liquidity-provider token accounts",
        accounts,
        &[&provider[0], &provider[1]],
    )
    .await?;
    let mut accounts = Vec::new();
    accounts.extend(token::create_token_account_instructions(
        &payer,
        &trader[0],
        &hooked.pubkey(),
        &payer,
        hooked_features,
    ));
    accounts.extend(token::create_token_account_instructions(
        &payer,
        &trader[1],
        &quote.pubkey(),
        &payer,
        quote_features,
    ));
    send_step(
        chain,
        rec,
        "create trader token accounts",
        accounts,
        &[&trader[0], &trader[1]],
    )
    .await?;
    send_step(
        chain,
        rec,
        "fund provider and trader accounts",
        vec![
            token::mint_to_instruction(
                &hooked.pubkey(),
                &provider[0].pubkey(),
                &payer,
                PROVIDER_FUNDS,
            ),
            token::mint_to_instruction(
                &quote.pubkey(),
                &provider[1].pubkey(),
                &payer,
                PROVIDER_FUNDS,
            ),
            token::mint_to_instruction(&hooked.pubkey(), &trader[0].pubkey(), &payer, TRADER_FUNDS),
            token::mint_to_instruction(&quote.pubkey(), &trader[1].pubkey(), &payer, TRADER_FUNDS),
        ],
        &[],
    )
    .await?;
    Ok(World {
        hooked,
        quote,
        provider,
        trader,
    })
}

/// The evidence label of a mint-creation step.
fn describe_mint(role: &str, features: MintFeatures) -> String {
    let mut extensions = Vec::new();
    if features.hook {
        extensions.push("TransferHook".to_string());
    }
    if features.transfer_fee_bps > 0 {
        extensions.push(format!("TransferFee {} bps", features.transfer_fee_bps));
    }
    if extensions.is_empty() {
        return format!("create plain Token-2022 {role} mint");
    }
    let suffix = if features.hook {
        ", hook not yet enabled"
    } else {
        ""
    };
    format!(
        "create {role} Token-2022 mint ({} extension{suffix})",
        extensions.join(" + ")
    )
}
