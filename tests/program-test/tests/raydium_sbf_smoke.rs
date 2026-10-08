use solana_program_test::ProgramTest;
use solana_sdk::pubkey;

const CPMM_PROGRAM_ID: solana_sdk::pubkey::Pubkey =
    pubkey!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");
const CLMM_PROGRAM_ID: solana_sdk::pubkey::Pubkey =
    pubkey!("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK");

#[tokio::test]
#[ignore = "requires SBF_OUT_DIR=target/localnet-sbf after `cargo xtask localnet build`"]
async fn loads_external_hook_aware_cpmm_and_clmm_sbf_programs() {
    let mut program_test = ProgramTest::default();
    program_test.add_program("raydium_cp_swap", CPMM_PROGRAM_ID, None);
    program_test.add_program("raydium_clmm", CLMM_PROGRAM_ID, None);

    let context = program_test.start_with_context().await;
    for program_id in [CPMM_PROGRAM_ID, CLMM_PROGRAM_ID] {
        let account = context
            .banks_client
            .get_account(program_id)
            .await
            .expect("SBF program account query must succeed")
            .expect("ProgramTest must register the Raydium SBF artifact");
        assert!(account.executable, "registered program must be executable");
    }
}
