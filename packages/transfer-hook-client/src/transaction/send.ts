import type { Commitment, Connection, VersionedTransaction } from '@solana/web3.js';

export interface SendResult {
  signature: string;
  slot: number;
}

/**
 * Submit a signed v0 transaction and wait until it is confirmed. The wallet signs between simulation and
 * this call; nothing here signs. Throws if the transaction lands with an error.
 */
export async function sendAndConfirm(
  connection: Pick<Connection, 'sendRawTransaction' | 'confirmTransaction' | 'getLatestBlockhash'>,
  signed: VersionedTransaction,
  commitment: Commitment = 'confirmed'
): Promise<SendResult> {
  const latest = await connection.getLatestBlockhash(commitment);
  const signature = await connection.sendRawTransaction(signed.serialize(), { skipPreflight: true, maxRetries: 3 });
  const result = await connection.confirmTransaction({ signature, ...latest }, commitment);
  if (result.value.err) {
    throw new Error(`transaction ${signature} failed: ${JSON.stringify(result.value.err)}`);
  }
  return { signature, slot: result.context.slot };
}
