import { type AccountMeta, PublicKey, TransactionInstruction } from '@solana/web3.js';
import type { ResolvedLeg } from '../hook/resolve-leg.ts';
import { type LegLayout, frameAccounts, u16le, u64le } from './frame.ts';

/** `SHA256("global:swap_base_input")[..8]`, the live Raydium CPMM instruction. */
export const CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR = Buffer.from([143, 190, 90, 218, 196, 30, 51, 222]);
/** `SHA256("global:swap_base_input_v2")[..8]`, the hook-aware instruction. */
export const CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR = Buffer.from([179, 135, 209, 217, 135, 75, 40, 58]);
export const CPMM_SWAP_BASE_OUTPUT_V1_DISCRIMINATOR = Buffer.from([55, 217, 98, 86, 163, 74, 180, 173]);
export const CPMM_SWAP_BASE_OUTPUT_V2_DISCRIMINATOR = Buffer.from([29, 143, 223, 109, 3, 111, 151, 147]);

export const CPMM_SWAP_FIXED_ACCOUNTS = 13;

/** Fixed accounts of CPMM `swap_base_input` / `swap_base_input_v2`, in program order. */
export interface CpmmSwapAccounts {
  payer: PublicKey;
  authority: PublicKey;
  ammConfig: PublicKey;
  poolState: PublicKey;
  inputTokenAccount: PublicKey;
  outputTokenAccount: PublicKey;
  inputVault: PublicKey;
  outputVault: PublicKey;
  inputTokenProgram: PublicKey;
  outputTokenProgram: PublicKey;
  inputTokenMint: PublicKey;
  outputTokenMint: PublicKey;
  observationState: PublicKey;
}

export const CPMM_INPUT_LAYOUT: LegLayout = { source: 4, destination: 6, authority: 0, mint: 10 };
export const CPMM_OUTPUT_LAYOUT: LegLayout = { source: 7, destination: 5, authority: 1, mint: 11 };

const ro = (pubkey: PublicKey, isSigner = false): AccountMeta => ({ pubkey, isSigner, isWritable: false });
const rw = (pubkey: PublicKey): AccountMeta => ({ pubkey, isSigner: false, isWritable: true });

/** The 13 fixed metas in program order with the program's flags. */
export function cpmmFixedMetas(a: CpmmSwapAccounts): AccountMeta[] {
  return [
    ro(a.payer, true),
    ro(a.authority),
    ro(a.ammConfig),
    rw(a.poolState),
    rw(a.inputTokenAccount),
    rw(a.outputTokenAccount),
    rw(a.inputVault),
    rw(a.outputVault),
    ro(a.inputTokenProgram),
    ro(a.outputTokenProgram),
    ro(a.inputTokenMint),
    ro(a.outputTokenMint),
    rw(a.observationState),
  ];
}

/** The unframed V1 `swap_base_input` (no hook accounts), the baseline framing is tested against. */
export function buildCpmmSwapBaseInputV1(
  programId: PublicKey,
  accounts: CpmmSwapAccounts,
  amountIn: bigint,
  minimumAmountOut: bigint
): TransactionInstruction {
  return new TransactionInstruction({
    programId,
    keys: cpmmFixedMetas(accounts),
    data: Buffer.concat([CPMM_SWAP_BASE_INPUT_V1_DISCRIMINATOR, u64le(amountIn), u64le(minimumAmountOut)]),
  });
}

function buildV2(
  discriminator: Buffer,
  programId: PublicKey,
  accounts: CpmmSwapAccounts,
  first: bigint,
  second: bigint,
  input: ResolvedLeg,
  output: ResolvedLeg
): TransactionInstruction {
  const framed = frameAccounts(cpmmFixedMetas(accounts), [], input, output, CPMM_INPUT_LAYOUT, CPMM_OUTPUT_LAYOUT);
  return new TransactionInstruction({
    programId,
    keys: framed.accounts,
    data: Buffer.concat([discriminator, u64le(first), u64le(second), u16le(framed.inputCount), u16le(framed.outputCount)]),
  });
}

/**
 * `swap_base_input_v2`: spend exactly `amountIn`, receive at least `minimumAmountOut`. The data is the
 * V1 data with the discriminator swapped and two `u16` slice lengths appended; the accounts are the
 * thirteen fixed ones, then the input leg's hook slice, then the output leg's.
 */
export function buildCpmmSwapBaseInputV2(
  programId: PublicKey,
  accounts: CpmmSwapAccounts,
  amountIn: bigint,
  minimumAmountOut: bigint,
  input: ResolvedLeg,
  output: ResolvedLeg
): TransactionInstruction {
  return buildV2(CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR, programId, accounts, amountIn, minimumAmountOut, input, output);
}

/** `swap_base_output_v2`: receive exactly `amountOut`, spend at most `maxAmountIn`. */
export function buildCpmmSwapBaseOutputV2(
  programId: PublicKey,
  accounts: CpmmSwapAccounts,
  maxAmountIn: bigint,
  amountOut: bigint,
  input: ResolvedLeg,
  output: ResolvedLeg
): TransactionInstruction {
  return buildV2(CPMM_SWAP_BASE_OUTPUT_V2_DISCRIMINATOR, programId, accounts, maxAmountIn, amountOut, input, output);
}

/** The CPMM pool authority PDA (seed `vault_and_lp_mint_auth_seed`); it signs every output leg. */
export function getCpmmPoolAuthority(programId: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync([Buffer.from('vault_and_lp_mint_auth_seed')], programId)[0];
}
