//! The mints and funded accounts every flow starts from.

use solana_sdk::signature::{Keypair, Signer};

use super::{recorder::Recorder, support::*};
use crate::{
    chain::{Chain, Result},
    token,
};

const DECIMALS: u8 = 6;
pub(super) const PROVIDER_FUNDS: u64 = 2_000_000_000;
pub(super) const TRADER_FUNDS: u64 = 10_000;

/// The two mints (the hooked one is the smaller pubkey, hence `mint_0`) and the funded accounts.
pub(super) struct World {
    pub(super) hooked: Keypair,
    pub(super) quote: Keypair,
    pub(super) provider: [Keypair; 2],
    pub(super) trader: [Keypair; 2],
}

pub(super) async fn create_world<C: Chain>(chain: &mut C, rec: &mut Recorder) -> Result<World> {
    let payer = chain.payer().pubkey();
    let (a, b) = (Keypair::new(), Keypair::new());
    let (hooked, quote) = if a.pubkey() < b.pubkey() {
        (a, b)
    } else {
        (b, a)
    };
    let provider = [Keypair::new(), Keypair::new()];
    let trader = [Keypair::new(), Keypair::new()];

    send_step(
        chain,
        rec,
        "create hooked Token-2022 mint (TransferHook extension, hook not yet enabled)",
        token::create_mint_instructions(&payer, &hooked, &payer, DECIMALS, true),
        &[&hooked],
    )
    .await?;
    send_step(
        chain,
        rec,
        "create plain Token-2022 quote mint",
        token::create_mint_instructions(&payer, &quote, &payer, DECIMALS, false),
        &[&quote],
    )
    .await?;
    let mut accounts = Vec::new();
    accounts.extend(token::create_token_account_instructions(
        &payer,
        &provider[0],
        &hooked.pubkey(),
        &payer,
        true,
    ));
    accounts.extend(token::create_token_account_instructions(
        &payer,
        &provider[1],
        &quote.pubkey(),
        &payer,
        false,
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
        true,
    ));
    accounts.extend(token::create_token_account_instructions(
        &payer,
        &trader[1],
        &quote.pubkey(),
        &payer,
        false,
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
