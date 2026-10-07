//! `env probe`: does each Raydium program in an environment recognise the hook-aware instructions?
//!
//! Nothing is sent. For each instruction it simulates a call with the right discriminator and no
//! accounts. A program that has the instruction fails while checking accounts; one that does not
//! fails with Anchor's `InstructionFallbackNotFound` (error 101). So the probe tells our
//! hook-aware forks from Raydium's own programs, and will tell the day Raydium ships them, without
//! the hook author changing anything: point the environment at the official ids and probe again.

use raydium_hook_driver::chain::Chain;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use transfer_hook_sdk::{
    CLMM_SWAP_V2_DISCRIMINATOR, CLMM_SWAP_V3_DISCRIMINATOR, CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR,
    CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR,
};

use super::{load_env, rpc_chain};
use crate::args::{Flags, Res};

/// What a probe of one instruction found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Finding {
    /// The program has this instruction (it got as far as checking accounts).
    Recognised,
    /// Anchor's `InstructionFallbackNotFound`: no such instruction.
    NotRecognised,
    /// The program is not deployed, or not executable.
    NoProgram,
    /// Something else: shown as it was.
    Other,
}

/// Classify the logs of a probe simulation.
pub(crate) fn classify(logs: &[String]) -> Finding {
    if logs
        .iter()
        .any(|l| l.contains("InstructionFallbackNotFound") || l.contains("Error Number: 101"))
    {
        Finding::NotRecognised
    } else if logs.iter().any(|l| l.contains("invoke [1]")) {
        Finding::Recognised
    } else {
        Finding::Other
    }
}

async fn probe_one<C: Chain>(
    chain: &mut C,
    program: Pubkey,
    discriminator: [u8; 8],
) -> Result<Finding, String> {
    let present = chain.account(&program).await.map_err(|e| e.to_string())?;
    if !present.is_some_and(|a| a.executable) {
        return Ok(Finding::NoProgram);
    }
    // The discriminator, then zeroed arguments long enough for any of these instructions.
    let mut data = discriminator.to_vec();
    data.extend_from_slice(&[0u8; 64]);
    let sim = chain
        .simulate(
            &[Instruction {
                program_id: program,
                accounts: vec![],
                data,
            }],
            &[],
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(classify(&sim.logs))
}

fn label(finding: Finding) -> &'static str {
    match finding {
        Finding::Recognised => "recognised",
        Finding::NotRecognised => "NOT recognised",
        Finding::NoProgram => "program not deployed",
        Finding::Other => "inconclusive",
    }
}

pub(crate) async fn probe(flags: &Flags) -> Res<()> {
    let (_, env) = load_env(flags)?;
    let mut chain = rpc_chain(&env, flags)?;
    println!("environment {} ({})", env.name, env.cluster);
    let mut any = false;
    for (name, program, instructions) in [
        (
            "cpmm",
            env.cpmm_program().ok(),
            [
                (
                    "swap_base_input (V1, the control)",
                    CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR,
                ),
                (
                    "swap_base_input_v2 (hook-aware)",
                    CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR,
                ),
            ],
        ),
        (
            "clmm",
            env.clmm_program().ok(),
            [
                ("swap_v2 (V1, the control)", CLMM_SWAP_V2_DISCRIMINATOR),
                ("swap_v3 (hook-aware)", CLMM_SWAP_V3_DISCRIMINATOR),
            ],
        ),
    ] {
        let Some(program) = program else { continue };
        any = true;
        println!("\n{name}  {program}");
        let mut findings = Vec::new();
        for (what, discriminator) in instructions {
            let finding = probe_one(&mut chain, program, discriminator).await?;
            println!("  {what:<36} {}", label(finding));
            findings.push(finding);
        }
        match (findings[0], findings[1]) {
            (_, Finding::Recognised) => {
                println!("  => hook-aware: this program accepts per-leg hook accounts")
            }
            (Finding::Recognised, Finding::NotRecognised) => println!(
                "  => NOT hook-aware: the program has the V1 instruction but not the hook-aware one; the driver will not send it hooked swaps"
            ),
            _ => println!("  => inconclusive"),
        }
    }
    if !any {
        return Err("the environment lists no CPMM or CLMM program".into());
    }
    println!("\nNothing was sent: these were simulations of malformed calls.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logs(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn an_unknown_instruction_is_not_recognised() {
        let found = classify(&logs(&[
            "Program P invoke [1]",
            "Program log: AnchorError occurred. Error Code: InstructionFallbackNotFound. Error Number: 101. Error Message: Fallback functions are not supported.",
            "Program P failed: custom program error: 0x65",
        ]));
        assert_eq!(found, Finding::NotRecognised);
    }

    #[test]
    fn a_known_instruction_that_fails_on_accounts_is_recognised() {
        let found = classify(&logs(&[
            "Program P invoke [1]",
            "Program log: Instruction: SwapBaseInputV2",
            "Program log: AnchorError occurred. Error Code: AccountNotEnoughKeys. Error Number: 3005.",
            "Program P failed: custom program error: 0xbbd",
        ]));
        assert_eq!(found, Finding::Recognised);
    }

    #[test]
    fn no_logs_at_all_is_inconclusive() {
        assert_eq!(classify(&[]), Finding::Other);
    }
}
