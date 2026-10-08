import { type AccountMeta, PublicKey, TransactionInstruction } from '@solana/web3.js';
import { HookClientError } from '../hook/errors.ts';
import type { ResolvedLeg } from '../hook/resolve-leg.ts';
import { type LegLayout, frameAccounts, u128le, u16le, u64le } from './frame.ts';

export const CLMM_SWAP_V2_DISCRIMINATOR = Buffer.from([43, 4, 237, 11, 26, 201, 30, 98]);
export const CLMM_SWAP_V3_DISCRIMINATOR = Buffer.from([240, 224, 38, 33, 176, 31, 241, 175]);
export const CLMM_SWAP_FIXED_ACCOUNTS = 13;

/** Fixed accounts of CLMM `swap_v2` / `swap_v3` (`SwapSingleV2`), in program order. */
export interface ClmmSwapAccounts {
  payer: PublicKey;
  ammConfig: PublicKey;
  poolState: PublicKey;
  inputTokenAccount: PublicKey;
  outputTokenAccount: PublicKey;
  inputVault: PublicKey;
  outputVault: PublicKey;
  observationState: PublicKey;
  tokenProgram: PublicKey;
  tokenProgram2022: PublicKey;
  memoProgram: PublicKey;
  inputVaultMint: PublicKey;
  outputVaultMint: PublicKey;
}

export interface ClmmSwapArgs {
  amount: bigint;
  otherAmountThreshold: bigint;
  sqrtPriceLimitX64: bigint;
  isBaseInput: boolean;
}

export const CLMM_INPUT_LAYOUT: LegLayout = { source: 3, destination: 5, authority: 0, mint: 11 };
/** The output leg is signed by the pool, which is account 2. */
export const CLMM_OUTPUT_LAYOUT: LegLayout = { source: 6, destination: 4, authority: 2, mint: 12 };

const ro = (pubkey: PublicKey, isSigner = false): AccountMeta => ({ pubkey, isSigner, isWritable: false });
const rw = (pubkey: PublicKey): AccountMeta => ({ pubkey, isSigner: false, isWritable: true });

export function clmmFixedMetas(a: ClmmSwapAccounts): AccountMeta[] {
  return [
    ro(a.payer, true),
    ro(a.ammConfig),
    rw(a.poolState),
    rw(a.inputTokenAccount),
    rw(a.outputTokenAccount),
    rw(a.inputVault),
    rw(a.outputVault),
    rw(a.observationState),
    ro(a.tokenProgram),
    ro(a.tokenProgram2022),
    ro(a.memoProgram),
    ro(a.inputVaultMint),
    ro(a.outputVaultMint),
  ];
}

/** Tick arrays (writable) then the optional bitmap extension (marked writable, a superset of need). */
function remaining(tickArrays: readonly PublicKey[], bitmapExtension: PublicKey | null): AccountMeta[] {
  return [...tickArrays.map(rw), ...(bitmapExtension ? [rw(bitmapExtension)] : [])];
}

function argsData(args: ClmmSwapArgs): Buffer {
  return Buffer.concat([
    u64le(args.amount),
    u64le(args.otherAmountThreshold),
    u128le(args.sqrtPriceLimitX64),
    Buffer.from([args.isBaseInput ? 1 : 0]),
  ]);
}

/** The unframed `swap_v2` (no hook accounts), the baseline framing is tested against. */
export function buildClmmSwapV2(
  programId: PublicKey,
  accounts: ClmmSwapAccounts,
  tickArrays: readonly PublicKey[],
  bitmapExtension: PublicKey | null,
  args: ClmmSwapArgs
): TransactionInstruction {
  return new TransactionInstruction({
    programId,
    keys: [...clmmFixedMetas(accounts), ...remaining(tickArrays, bitmapExtension)],
    data: Buffer.concat([CLMM_SWAP_V2_DISCRIMINATOR, argsData(args)]),
  });
}

/**
 * `swap_v3`: `swap_v2` data with four `u16` counts appended (tick arrays, bitmap extension, input hook
 * accounts, output hook accounts). The accounts are the thirteen fixed ones, the tick arrays, the
 * optional bitmap, then the input leg's hook slice, then the output leg's.
 */
export function buildClmmSwapV3(
  programId: PublicKey,
  accounts: ClmmSwapAccounts,
  tickArrays: readonly PublicKey[],
  bitmapExtension: PublicKey | null,
  args: ClmmSwapArgs,
  input: ResolvedLeg,
  output: ResolvedLeg
): TransactionInstruction {
  if (tickArrays.length > 0xffff) throw new HookClientError('bad-instruction-input', 'too many tick arrays');
  const framed = frameAccounts(
    clmmFixedMetas(accounts),
    remaining(tickArrays, bitmapExtension),
    input,
    output,
    CLMM_INPUT_LAYOUT,
    CLMM_OUTPUT_LAYOUT
  );
  return new TransactionInstruction({
    programId,
    keys: framed.accounts,
    data: Buffer.concat([
      CLMM_SWAP_V3_DISCRIMINATOR,
      argsData(args),
      u16le(tickArrays.length),
      u16le(bitmapExtension ? 1 : 0),
      u16le(framed.inputCount),
      u16le(framed.outputCount),
    ]),
  });
}
