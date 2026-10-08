import {
  type HookEnvironment,
  type SimulationReport,
  compileV0,
  sendAndConfirm,
  simulateHookAwareTransaction,
} from '@raydium-transfer-hook/client';
import type { Connection, PublicKey, VersionedTransaction } from '@solana/web3.js';
import type { PoolContext } from './chain.ts';
import { type FailureView, presentClientError, presentSimulationFailure } from './present.ts';
import type { SwapQuote } from './quote.ts';
import type { PreparedSwap } from './swap.ts';

export type SwapPhase = 'preparing' | 'simulating' | 'awaiting-signature' | 'confirming';

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
    const launchMint = context.launch ? (context.launch.hookedSide === 'A' ? context.pool.tokenA.mint : context.pool.tokenB.mint) : null;
    prepared = await context.adapter.buildSwap({
      connection,
      environment,
      pool: context.pool,
      quote,
      inputIsA: request.inputIsA,
      amountIn: request.amountIn,
      payer: request.payer,
      launchMint,
    });
  } catch (error) {
    return { status: 'blocked', failure: presentClientError(error) };
  }

  phase('simulating');
  let simulation: SimulationReport;
  try {
    const { blockhash } = await connection.getLatestBlockhash('confirmed');
    const unsigned = compileV0(request.payer, blockhash, prepared.instructions);
    const hookPrograms = [prepared.input.hookProgram, prepared.output.hookProgram].filter((key) => key !== null);
    simulation = await simulateHookAwareTransaction(connection, unsigned, hookPrograms);
    if (!simulation.ok) {
      const failure = simulation.failure ?? { kind: 'other' as const, message: 'the simulation failed' };
      return {
        status: 'blocked',
        failure: presentSimulationFailure(failure, { fairLaunchProgramId: environment.fairLaunchProgramId }, simulation.logs),
        details: { quote, prepared, simulation },
      };
    }

    phase('awaiting-signature');
    const fresh = await connection.getLatestBlockhash('confirmed');
    const signed = await request.signTransaction(compileV0(request.payer, fresh.blockhash, prepared.instructions));

    phase('confirming');
    const sent = await sendAndConfirm(connection, signed);
    return { status: 'success', signature: sent.signature, slot: sent.slot, details: { quote, prepared, simulation } };
  } catch (error) {
    return { status: 'blocked', failure: presentClientError(error), details: { quote, prepared } };
  }
}
