import {
  type Commitment,
  type Connection,
  type PublicKey,
  type SimulatedTransactionResponse,
  type TransactionInstruction,
  TransactionMessage,
  VersionedTransaction,
} from '@solana/web3.js';
import { type HookFailure, decodeTransferHookFailure } from '../hook/errors.ts';

/** A v0 transaction, unsigned, ready for simulation and then the wallet. */
export function compileV0(
  payer: PublicKey,
  recentBlockhash: string,
  instructions: readonly TransactionInstruction[]
): VersionedTransaction {
  const message = new TransactionMessage({ payerKey: payer, recentBlockhash, instructions: [...instructions] }).compileToV0Message();
  return new VersionedTransaction(message);
}

export interface SimulationReport {
  ok: boolean;
  unitsConsumed: number | null;
  logs: string[];
  /** Set when the transaction failed; says whether a hook, a program or something else refused it. */
  failure: HookFailure | null;
  raw: SimulatedTransactionResponse;
}

/**
 * Simulate an unsigned v0 transaction without verifying signatures (the wallet has not signed yet) and
 * with the latest blockhash, then decode a failure. Nothing is submitted.
 */
export async function simulateHookAwareTransaction(
  connection: Pick<Connection, 'simulateTransaction'>,
  transaction: VersionedTransaction,
  hookPrograms: readonly PublicKey[],
  commitment: Commitment = 'confirmed'
): Promise<SimulationReport> {
  const { value } = await connection.simulateTransaction(transaction, {
    sigVerify: false,
    replaceRecentBlockhash: true,
    commitment,
  });
  const logs = value.logs ?? [];
  return {
    ok: value.err === null,
    unitsConsumed: value.unitsConsumed ?? null,
    logs,
    failure: value.err === null ? null : decodeTransferHookFailure({ err: value.err, logs }, hookPrograms),
    raw: value,
  };
}
