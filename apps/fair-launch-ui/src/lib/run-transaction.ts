import {
  type SimulationReport,
  compileV0,
  sendAndConfirm,
  simulateHookAwareTransaction,
} from '@raydium-transfer-hook/client';
import type { Connection, PublicKey, TransactionInstruction, VersionedTransaction } from '@solana/web3.js';
import { type FailureView, type PresentContext, presentClientError, presentSimulationFailure } from './present.ts';

export type TransactionPhase = 'simulating' | 'awaiting-signature' | 'confirming';

export interface TransactionRequest {
  connection: Connection;
  payer: PublicKey;
  instructions: readonly TransactionInstruction[];
  /** Hook programs whose own errors should be told apart from other programs' (the innermost failing program wins). */
  hookPrograms: readonly PublicKey[];
  signTransaction: (transaction: VersionedTransaction) => Promise<VersionedTransaction>;
  present: PresentContext;
  onPhase?: (phase: TransactionPhase) => void;
}

export type TransactionOutcome =
  /** Simulation or something before it refused the transaction; nothing was signed or sent. */
  | { status: 'blocked'; failure: FailureView; simulation?: SimulationReport }
  | { status: 'success'; signature: string; slot: number; simulation: SimulationReport };

/**
 * Compile a v0 transaction, simulate it, and only if the simulation passes ask the wallet to sign, then
 * send and confirm. A refusal at any step comes back as a readable failure with nothing submitted.
 */
export async function runTransaction(request: TransactionRequest): Promise<TransactionOutcome> {
  const { connection, payer } = request;
  const phase = (value: TransactionPhase) => request.onPhase?.(value);
  try {
    phase('simulating');
    const { blockhash } = await connection.getLatestBlockhash('confirmed');
    const simulation = await simulateHookAwareTransaction(
      connection,
      compileV0(payer, blockhash, request.instructions),
      request.hookPrograms
    );
    if (!simulation.ok) {
      const failure = simulation.failure ?? { kind: 'other' as const, message: 'the simulation failed' };
      return { status: 'blocked', failure: presentSimulationFailure(failure, request.present, simulation.logs), simulation };
    }

    phase('awaiting-signature');
    const fresh = await connection.getLatestBlockhash('confirmed');
    const signed = await request.signTransaction(compileV0(payer, fresh.blockhash, request.instructions));

    phase('confirming');
    const sent = await sendAndConfirm(connection, signed);
    return { status: 'success', signature: sent.signature, slot: sent.slot, simulation };
  } catch (error) {
    return { status: 'blocked', failure: presentClientError(error) };
  }
}
