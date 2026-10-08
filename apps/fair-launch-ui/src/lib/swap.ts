import {
  type ClmmSwapAccounts,
  type CpmmSwapAccounts,
  type HookEnvironment,
  type ResolvedLeg,
  buildClmmSwapV2,
  buildClmmSwapV3,
  buildCpmmSwapBaseInputV1,
  buildCpmmSwapBaseInputV2,
  getFairLaunchCounterAddress,
  programKeys,
  resolveTransferHookLeg,
} from '@raydium-transfer-hook/client';
import {
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountIdempotentInstruction,
  getAssociatedTokenAddressSync,
} from '@solana/spl-token';
import { ComputeBudgetProgram, type Connection, PublicKey, type TransactionInstruction } from '@solana/web3.js';
import type { PoolView } from './pool.ts';
import type { SwapQuote } from './quote.ts';

/** Compute units asked for; two hooked legs measured well under this (see docs/commercial-and-limits.md). */
export const SWAP_COMPUTE_UNIT_LIMIT = 400_000;

export const MEMO_PROGRAM_ID = new PublicKey('MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr');

export interface PrepareSwapInput {
  connection: Pick<Connection, 'getAccountInfo' | 'getMultipleAccountsInfo'>;
  environment: HookEnvironment;
  pool: PoolView;
  quote: SwapQuote;
  inputIsA: boolean;
  amountIn: bigint;
  payer: PublicKey;
  /** The mint of the fair-launch token in this pool, if any; its hook program must be this environment's. */
  launchMint: PublicKey | null;
}

export type InstructionLabel = 'swap_base_input_v2' | 'swap_base_input' | 'swap_v3' | 'swap_v2';

export interface PreparedSwap {
  instructions: TransactionInstruction[];
  /** Which Raydium instruction the swap uses. */
  label: InstructionLabel;
  kind: 'cpmm' | 'clmm';
  input: ResolvedLeg;
  output: ResolvedLeg;
  userInputAccount: PublicKey;
  userOutputAccount: PublicKey;
}

/**
 * Resolve both legs' hook accounts and build the hook-aware swap for the pool's AMM. The stock Raydium
 * builders are never used: they do not know the `_v2` / `swap_v3` instructions or the hook-account counts.
 */
export async function prepareSwap(input: PrepareSwapInput): Promise<PreparedSwap> {
  const { connection, environment, pool, payer, quote } = input;
  const programs = programKeys(environment);
  const tokenIn = input.inputIsA ? pool.tokenA : pool.tokenB;
  const tokenOut = input.inputIsA ? pool.tokenB : pool.tokenA;
  const userInputAccount = getAssociatedTokenAddressSync(tokenIn.mint, payer, false, tokenIn.tokenProgram);
  const userOutputAccount = getAssociatedTokenAddressSync(tokenOut.mint, payer, false, tokenOut.tokenProgram);

  const options = (mint: PublicKey) => {
    const isLaunch = input.launchMint?.equals(mint) ?? false;
    return isLaunch
      ? {
          expectedHookProgram: programs.fairLaunch,
          allowWritable: [getFairLaunchCounterAddress(mint, programs.fairLaunch)],
        }
      : {};
  };

  const [inputLeg, outputLeg] = await Promise.all([
    resolveTransferHookLeg(
      connection,
      { role: 'input', mint: tokenIn.mint, source: userInputAccount, destination: tokenIn.vault, authority: payer, amount: input.amountIn },
      options(tokenIn.mint)
    ),
    resolveTransferHookLeg(
      connection,
      {
        role: 'output',
        mint: tokenOut.mint,
        source: tokenOut.vault,
        destination: userOutputAccount,
        authority: pool.authority,
        amount: quote.minimumOut,
      },
      options(tokenOut.mint)
    ),
  ]);
  const hooked = inputLeg.hookProgram !== null || outputLeg.hookProgram !== null;

  let swap: TransactionInstruction;
  let label: InstructionLabel;
  if (pool.kind === 'cpmm') {
    const accounts: CpmmSwapAccounts = {
      payer,
      authority: pool.authority,
      ammConfig: pool.ammConfig,
      poolState: pool.poolId,
      inputTokenAccount: userInputAccount,
      outputTokenAccount: userOutputAccount,
      inputVault: tokenIn.vault,
      outputVault: tokenOut.vault,
      inputTokenProgram: tokenIn.tokenProgram,
      outputTokenProgram: tokenOut.tokenProgram,
      inputTokenMint: tokenIn.mint,
      outputTokenMint: tokenOut.mint,
      observationState: pool.observation,
    };
    swap = hooked
      ? buildCpmmSwapBaseInputV2(programs.cpmm, accounts, input.amountIn, quote.minimumOut, inputLeg, outputLeg)
      : buildCpmmSwapBaseInputV1(programs.cpmm, accounts, input.amountIn, quote.minimumOut);
    label = hooked ? 'swap_base_input_v2' : 'swap_base_input';
  } else {
    const accounts: ClmmSwapAccounts = {
      payer,
      ammConfig: pool.ammConfig,
      poolState: pool.poolId,
      inputTokenAccount: userInputAccount,
      outputTokenAccount: userOutputAccount,
      inputVault: tokenIn.vault,
      outputVault: tokenOut.vault,
      observationState: pool.observation,
      tokenProgram: TOKEN_PROGRAM_ID,
      tokenProgram2022: TOKEN_2022_PROGRAM_ID,
      memoProgram: MEMO_PROGRAM_ID,
      inputVaultMint: tokenIn.mint,
      outputVaultMint: tokenOut.mint,
    };
    // A zero price limit means "no limit" to the program.
    const args = { amount: input.amountIn, otherAmountThreshold: quote.minimumOut, sqrtPriceLimitX64: 0n, isBaseInput: true };
    swap = hooked
      ? buildClmmSwapV3(programs.clmm, accounts, quote.tickArrays, quote.bitmapExtension, args, inputLeg, outputLeg)
      : buildClmmSwapV2(programs.clmm, accounts, quote.tickArrays, quote.bitmapExtension, args);
    label = hooked ? 'swap_v3' : 'swap_v2';
  }

  return {
    instructions: [
      ComputeBudgetProgram.setComputeUnitLimit({ units: SWAP_COMPUTE_UNIT_LIMIT }),
      createAssociatedTokenAccountIdempotentInstruction(payer, userOutputAccount, payer, tokenOut.mint, tokenOut.tokenProgram),
      swap,
    ],
    label,
    kind: pool.kind,
    input: inputLeg,
    output: outputLeg,
    userInputAccount,
    userOutputAccount,
  };
}
