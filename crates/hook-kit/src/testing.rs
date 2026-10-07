//! In-process test helpers (feature `test-support`): a Token-2022 mint whose TransferHook points at
//! the hook under test, funded token accounts, and transfers whose hook accounts are resolved
//! through the SDK exactly as an integrator would.
//!
//! The hook runs natively by default. Set `SBF_OUT_DIR` to a directory holding the program's
//! `.so` to run the real SBF build instead (`ProgramTest` prefers it).

use std::collections::HashMap;

use solana_program_test::{BanksClientError, ProgramTest, ProgramTestContext};
use solana_sdk::{
    instruction::{Instruction, InstructionError},
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    system_instruction,
    transaction::{Transaction, TransactionError},
};
use spl_token_2022::{
    extension::{
        transfer_hook::instruction as transfer_hook_instruction, ExtensionType, StateWithExtensions,
    },
    instruction as token_instruction,
    state::{Account as TokenAccount, Mint},
};
use transfer_hook_sdk::{
    default_allowed_loaders, resolve_leg, FetchError, LegRole, PrivilegePolicy, ResolveOptions,
    SplAccount, SplTransferLeg,
};

use crate::accounts::validation_list_address;

/// A token account to create: who owns it and its starting balance.
pub struct AccountSpec {
    /// `None`: owned by the test payer.
    pub owner: Option<Keypair>,
    pub balance: u64,
}

impl AccountSpec {
    pub fn payer_owned(balance: u64) -> Self {
        Self {
            owner: None,
            balance,
        }
    }

    pub fn owned_by(owner: Keypair, balance: u64) -> Self {
        Self {
            owner: Some(owner),
            balance,
        }
    }
}

/// A plain token made by [`World::create_plain_token`].
pub struct PlainToken {
    pub mint: Keypair,
    pub token_program: Pubkey,
    /// Owned by the payer, holds the whole supply.
    pub funder_account: Pubkey,
    /// One empty account per requested holder, owned by that holder.
    pub holder_accounts: Vec<Pubkey>,
}

pub struct World {
    pub context: ProgramTestContext,
    pub program_id: Pubkey,
    pub mint: Keypair,
    pub accounts: Vec<Keypair>,
    /// The signer that owns each account (a copy of the payer for payer-owned accounts).
    pub owners: Vec<Keypair>,
    /// The last transaction sent, to notice a resend (see [`World::send`]).
    last_signature: Option<Signature>,
}

fn clone_key(keypair: &Keypair) -> Keypair {
    Keypair::from_bytes(&keypair.to_bytes()).expect("keypair bytes")
}

/// Assert that `result` failed with the program's custom error `code`.
pub fn assert_custom_error(result: Result<(), BanksClientError>, code: u32) {
    match result {
        Err(BanksClientError::TransactionError(TransactionError::InstructionError(
            _,
            InstructionError::Custom(found),
        ))) => assert_eq!(found, code, "expected {code:#x}, got {found:#x}"),
        other => panic!("expected custom error {code:#x}, got {other:?}"),
    }
}

impl World {
    /// Start the bank with `test` (which must register the hook program) and create the hooked
    /// mint and one token account per `specs`. The mint's TransferHook points at `program_id`
    /// and its authority is the payer.
    pub async fn start(test: ProgramTest, program_id: Pubkey, specs: Vec<AccountSpec>) -> World {
        let context = test.start_with_context().await;
        let payer = context.payer.pubkey();
        let rent = context.banks_client.get_rent().await.unwrap();
        let mint = Keypair::new();
        let mint_len =
            ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::TransferHook])
                .unwrap();
        let account_len = ExtensionType::try_calculate_account_len::<TokenAccount>(&[
            ExtensionType::TransferHookAccount,
        ])
        .unwrap();

        let mut instructions = vec![
            system_instruction::create_account(
                &payer,
                &mint.pubkey(),
                rent.minimum_balance(mint_len),
                mint_len as u64,
                &spl_token_2022::id(),
            ),
            transfer_hook_instruction::initialize(
                &spl_token_2022::id(),
                &mint.pubkey(),
                Some(payer),
                Some(program_id),
            )
            .unwrap(),
            token_instruction::initialize_mint2(
                &spl_token_2022::id(),
                &mint.pubkey(),
                &payer,
                None,
                0,
            )
            .unwrap(),
        ];
        let mut accounts = Vec::new();
        let mut owners = Vec::new();
        for spec in &specs {
            let account = Keypair::new();
            let owner = spec
                .owner
                .as_ref()
                .map(clone_key)
                .unwrap_or_else(|| clone_key(&context.payer));
            instructions.push(system_instruction::create_account(
                &payer,
                &account.pubkey(),
                rent.minimum_balance(account_len),
                account_len as u64,
                &spl_token_2022::id(),
            ));
            instructions.push(
                token_instruction::initialize_account3(
                    &spl_token_2022::id(),
                    &account.pubkey(),
                    &mint.pubkey(),
                    &owner.pubkey(),
                )
                .unwrap(),
            );
            if spec.balance > 0 {
                instructions.push(
                    token_instruction::mint_to(
                        &spl_token_2022::id(),
                        &mint.pubkey(),
                        &account.pubkey(),
                        &payer,
                        &[],
                        spec.balance,
                    )
                    .unwrap(),
                );
            }
            accounts.push(account);
            owners.push(owner);
        }
        let world = World {
            context,
            program_id,
            mint,
            accounts,
            owners,
            last_signature: None,
        };
        let refs: Vec<&Keypair> = {
            let mut v: Vec<&Keypair> = vec![&world.mint];
            v.extend(world.accounts.iter());
            v
        };
        let blockhash = world
            .context
            .banks_client
            .get_latest_blockhash()
            .await
            .unwrap();
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer),
            &[&[&world.context.payer], refs.as_slice()].concat(),
            blockhash,
        );
        world
            .context
            .banks_client
            .process_transaction(tx)
            .await
            .expect("create the hooked mint and funded accounts");
        world
    }

    pub fn payer(&self) -> Pubkey {
        self.context.payer.pubkey()
    }

    pub fn account(&self, index: usize) -> Pubkey {
        self.accounts[index].pubkey()
    }

    pub fn owner(&self, index: usize) -> Pubkey {
        self.owners[index].pubkey()
    }

    /// Send a transaction signed by the payer and `extra_signers`.
    ///
    /// The bank treats a transaction identical to one it already processed (same blockhash, same
    /// instructions) as a duplicate and reports success without running it, which would make a
    /// "this must fail the second time" test pass or fail by timing. So a resend of the previous
    /// transaction first moves one slot ahead, keeping the clock, to get a fresh blockhash.
    pub async fn send(
        &mut self,
        instructions: &[Instruction],
        extra_signers: &[&Keypair],
    ) -> Result<(), BanksClientError> {
        let mut attempts = 0;
        loop {
            let blockhash = self
                .context
                .banks_client
                .get_latest_blockhash()
                .await
                .unwrap();
            let mut signers: Vec<&Keypair> = vec![&self.context.payer];
            signers.extend_from_slice(extra_signers);
            let tx = Transaction::new_signed_with_payer(
                instructions,
                Some(&self.context.payer.pubkey()),
                &signers,
                blockhash,
            );
            let signature = tx.signatures[0];
            if self.last_signature == Some(signature) && attempts < 5 {
                attempts += 1;
                let time = self.unix_time().await;
                let slot = self.current_slot().await;
                self.context.warp_to_slot(slot + 2).unwrap();
                self.set_unix_time(time).await;
                continue;
            }
            self.last_signature = Some(signature);
            return self.context.banks_client.process_transaction(tx).await;
        }
    }

    async fn current_slot(&mut self) -> u64 {
        let clock: solana_sdk::clock::Clock = self.context.banks_client.get_sysvar().await.unwrap();
        clock.slot
    }

    pub async fn data(&mut self, key: Pubkey) -> Vec<u8> {
        self.context
            .banks_client
            .get_account(key)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("account {key} does not exist"))
            .data
    }

    pub async fn try_data(&mut self, key: Pubkey) -> Option<Vec<u8>> {
        self.context
            .banks_client
            .get_account(key)
            .await
            .unwrap()
            .map(|a| a.data)
    }

    pub async fn balance(&mut self, index: usize) -> u64 {
        let data = self.data(self.account(index)).await;
        StateWithExtensions::<TokenAccount>::unpack(&data)
            .expect("token account")
            .base
            .amount
    }

    /// Set the cluster's unix time.
    pub async fn set_unix_time(&mut self, unix_timestamp: i64) {
        let mut clock: solana_sdk::clock::Clock =
            self.context.banks_client.get_sysvar().await.unwrap();
        clock.unix_timestamp = unix_timestamp;
        self.context.set_sysvar(&clock);
    }

    pub async fn unix_time(&mut self) -> i64 {
        let clock: solana_sdk::clock::Clock = self.context.banks_client.get_sysvar().await.unwrap();
        clock.unix_timestamp
    }

    /// Create a plain token (no hook) of either token program, with a payer-owned `funder_account`
    /// holding `supply` and one empty account per `holders` index, owned by that holder. For rules
    /// that pay out another token (rewards, spin-offs).
    pub async fn create_plain_token(
        &mut self,
        token_program: Pubkey,
        holders: &[usize],
        supply: u64,
    ) -> PlainToken {
        use solana_sdk::program_pack::Pack;
        let payer = self.payer();
        let rent = self.context.banks_client.get_rent().await.unwrap();
        let mint = Keypair::new();
        let mut instructions = vec![
            system_instruction::create_account(
                &payer,
                &mint.pubkey(),
                rent.minimum_balance(Mint::LEN),
                Mint::LEN as u64,
                &token_program,
            ),
            token_instruction::initialize_mint2(&token_program, &mint.pubkey(), &payer, None, 0)
                .unwrap(),
        ];
        let mut keypairs = vec![clone_key(&mint)];
        let mut accounts = Vec::new();
        let owners: Vec<Pubkey> = std::iter::once(payer)
            .chain(holders.iter().map(|i| self.owner(*i)))
            .collect();
        for owner in owners {
            let account = Keypair::new();
            instructions.push(system_instruction::create_account(
                &payer,
                &account.pubkey(),
                rent.minimum_balance(TokenAccount::LEN),
                TokenAccount::LEN as u64,
                &token_program,
            ));
            instructions.push(
                token_instruction::initialize_account3(
                    &token_program,
                    &account.pubkey(),
                    &mint.pubkey(),
                    &owner,
                )
                .unwrap(),
            );
            accounts.push(account.pubkey());
            keypairs.push(account);
        }
        instructions.push(
            token_instruction::mint_to(
                &token_program,
                &mint.pubkey(),
                &accounts[0],
                &payer,
                &[],
                supply,
            )
            .unwrap(),
        );
        let signers: Vec<&Keypair> = keypairs.iter().collect();
        self.send(&instructions, &signers)
            .await
            .expect("create the plain token and its accounts");
        PlainToken {
            mint,
            token_program,
            funder_account: accounts[0],
            holder_accounts: accounts[1..].to_vec(),
        }
    }

    /// The balance of any token account (either token program).
    pub async fn token_balance(&mut self, account: Pubkey) -> u64 {
        use solana_sdk::program_pack::Pack;
        let data = self.data(account).await;
        TokenAccount::unpack(&data[..TokenAccount::LEN])
            .expect("token account")
            .amount
    }

    /// Give the mint an end to minting (rules that assume a fixed supply need it).
    pub async fn revoke_mint_authority(&mut self) {
        let payer = self.payer();
        let ix = token_instruction::set_authority(
            &spl_token_2022::id(),
            &self.mint.pubkey(),
            None,
            token_instruction::AuthorityType::MintTokens,
            &payer,
            &[],
        )
        .unwrap();
        self.send(&[ix], &[]).await.expect("revoke mint authority");
    }

    /// A transfer of `amount` from account `from` to account `to` with the hook's accounts
    /// resolved through the SDK. Only the accounts in `allow_writable` may be writable extras.
    pub async fn transfer_ix(
        &mut self,
        from: usize,
        to: usize,
        amount: u64,
        allow_writable: &[Pubkey],
    ) -> Instruction {
        let mint = self.mint.pubkey();
        let mut fetched = HashMap::new();
        for key in [
            mint,
            validation_list_address(&mint, &self.program_id).0,
            self.program_id,
        ] {
            let account = self
                .context
                .banks_client
                .get_account(key)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("resolver account {key} does not exist"));
            fetched.insert(
                key,
                SplAccount {
                    key,
                    owner: account.owner,
                    data: account.data,
                    executable: account.executable,
                },
            );
        }
        // Natively, ProgramTest registers the hook under the native loader; allow it explicitly.
        let mut loaders = default_allowed_loaders();
        loaders.push(solana_sdk::native_loader::id());
        let mut options = ResolveOptions::default()
            .with_expected_hook_program(self.program_id)
            .with_allowed_loaders(loaders);
        if !allow_writable.is_empty() {
            options = options.with_privilege_policy(PrivilegePolicy::allowing_writable(
                allow_writable.iter().copied(),
            ));
        }
        let (source, destination, authority) =
            (self.account(from), self.account(to), self.owner(from));
        let leg = resolve_leg(
            LegRole::Input,
            SplTransferLeg {
                source,
                mint,
                destination,
                authority,
                amount,
            },
            &options,
            |key| {
                let account = fetched.get(&key).cloned();
                async move { Ok::<_, FetchError>(account) }
            },
        )
        .await
        .expect("resolve the hook's accounts with the SDK");
        let metas = leg.slice().expect("hooked leg").metas().to_vec();
        let mut transfer = token_instruction::transfer_checked(
            &spl_token_2022::id(),
            &source,
            &mint,
            &destination,
            &authority,
            &[],
            amount,
            0,
        )
        .unwrap();
        transfer.accounts.extend(metas);
        transfer
    }

    /// Transfer and send, signing with the source account's owner.
    pub async fn transfer(
        &mut self,
        from: usize,
        to: usize,
        amount: u64,
        allow_writable: &[Pubkey],
    ) -> Result<(), BanksClientError> {
        let ix = self.transfer_ix(from, to, amount, allow_writable).await;
        let owner = clone_key(&self.owners[from]);
        self.send(&[ix], &[&owner]).await
    }
}
