import type { HookEnvironment, SimulationReport } from '@raydium-transfer-hook/client';
import type { Connection, PublicKey, VersionedTransaction } from '@solana/web3.js';
import type { PoolContext } from './chain.ts';
import { hookedToken } from './hooks.ts';
import { type FailureView, presentClientError, presentContext } from './present.ts';
import type { SwapQuote } from './quote.ts';
import { type TransactionPhase, runTransaction } from './run-transaction.ts';
import type { PreparedSwap } from './swap.ts';

export type SwapPhase = 'preparing' | TransactionPhase;

export interface SwapRequest {
  connection: Connection;
  environment: HookEnvironment;
  payer: PublicKey;
  inputIsA: boolean;
  amountIn: bigint;
  slippageBps: number;
  /** Reload the pool and the launch policy right now, so the quote and the hook accounts are fresh. */
  reload: () => Promise<PoolContext>;
  signTransaction: (transaction: VersionedTransaction) => Promise<VersionedTransaction>;
  onPhase?: (phase: SwapPhase) => void;
}

export interface SwapDetails {
  quote: SwapQuote;
  prepared: PreparedSwap;
  simulation: SimulationReport;
}

export type SwapOutcome =
  /** Simulation or the client refused the swap; nothing was signed or sent. */
  | { status: 'blocked'; failure: FailureView; details?: Partial<SwapDetails> }
  | { status: 'success'; signature: string; slot: number; details: SwapDetails };

/**
 * The Swap button, in the order the plan fixes: reload everything, quote, resolve both hook slices, build
 * the hook-aware instruction, compile v0, simulate, and only if the simulation passes ask the wallet to
 * sign. A refusal at any step is returned as a human-readable failure and nothing is submitted.
 */
export async function runHookAwareSwap(request: SwapRequest): Promise<SwapOutcome> {
  const { connection, environment } = request;
  const phase = (value: SwapPhase) => request.onPhase?.(value);
  phase('preparing');
  let context: PoolContext;
  let quote: SwapQuote;
  let prepared: PreparedSwap;
  try {
    context = await request.reload();
    quote = context.adapter.quote(context.pool, request.inputIsA, request.amountIn, request.slippageBps);
    prepared = await context.adapter.buildSwap({
      connection,
      environment,
      pool: context.pool,
      quote,
      inputIsA: request.inputIsA,
      amountIn: request.amountIn,
      payer: request.payer,
      hooked: hookedToken(context),
    });
  } catch (error) {
    return { status: 'blocked', failure: presentClientError(error) };
  }

  const hookPrograms = [prepared.input.hookProgram, prepared.output.hookProgram].filter((key) => key !== null);
  const outcome = await runTransaction({
    connection,
    payer: request.payer,
    instructions: prepared.instructions,
    hookPrograms,
    signTransaction: request.signTransaction,
    present: presentContext(environment),
    onPhase: phase,
  });
  if (outcome.status === 'blocked') {
    return { status: 'blocked', failure: outcome.failure, details: { quote, prepared, simulation: outcome.simulation } };
  }
  return { status: 'success', signature: outcome.signature, slot: outcome.slot, details: { quote, prepared, simulation: outcome.simulation } };
}
