import { type Commitment, type Connection, PublicKey } from '@solana/web3.js';
import { HookClientError } from '../hook/errors.ts';
import { getFairLaunchConfigAddress, getFairLaunchCounterAddress } from './addresses.ts';
import type { FairLaunchConfig, FairLaunchCounter } from './types.ts';

export const MAX_VENUES = 4;
export const CONFIG_DISCRIMINATOR = Buffer.from('FLCONFIG', 'ascii');
export const COUNTER_DISCRIMINATOR = Buffer.from('FLCOUNTR', 'ascii');
export const CONFIG_LEN = 8 + 1 + 32 + 1 + 32 * MAX_VENUES + 8 + 8 + 8 + 8 + 4 + 8;
export const COUNTER_LEN = 8 + 1 + 8 + 4;

/** Decode a 214-byte config account. Throws if the length, discriminator or venue count is wrong. */
export function decodeFairLaunchConfig(data: Uint8Array): FairLaunchConfig {
  const buf = Buffer.from(data.buffer, data.byteOffset, data.byteLength);
  if (buf.length !== CONFIG_LEN || !buf.subarray(0, 8).equals(CONFIG_DISCRIMINATOR)) {
    throw new HookClientError('bad-instruction-input', 'not a fair-launch config account');
  }
  const venueCount = buf[41];
  if (venueCount === 0 || venueCount > MAX_VENUES) {
    throw new HookClientError('bad-instruction-input', `the fair-launch config lists ${venueCount} venues`);
  }
  const venues: PublicKey[] = [];
  for (let i = 0; i < venueCount; i += 1) {
    venues.push(new PublicKey(buf.subarray(42 + 32 * i, 74 + 32 * i)));
  }
  const at = 42 + 32 * MAX_VENUES;
  return {
    bump: buf[8],
    mint: new PublicKey(buf.subarray(9, 41)),
    venues,
    windowStart: buf.readBigInt64LE(at),
    windowEnd: buf.readBigInt64LE(at + 8),
    maxBuy: buf.readBigUInt64LE(at + 16),
    maxWallet: buf.readBigUInt64LE(at + 24),
    maxBuysPerSlot: buf.readUInt32LE(at + 32),
    maxPriorityMicroLamports: buf.readBigUInt64LE(at + 36),
  };
}

export function decodeFairLaunchCounter(data: Uint8Array): FairLaunchCounter {
  const buf = Buffer.from(data.buffer, data.byteOffset, data.byteLength);
  if (buf.length !== COUNTER_LEN || !buf.subarray(0, 8).equals(COUNTER_DISCRIMINATOR)) {
    throw new HookClientError('bad-instruction-input', 'not a fair-launch counter account');
  }
  return { bump: buf[8], slot: buf.readBigUInt64LE(9), buys: buf.readUInt32LE(17) };
}

/** Read and decode a mint's config; `null` if the account does not exist or the program does not own it. */
export async function readFairLaunchConfig(
  connection: Pick<Connection, 'getAccountInfo'>,
  mint: PublicKey,
  programId: PublicKey,
  commitment: Commitment = 'confirmed'
): Promise<FairLaunchConfig | null> {
  const account = await connection.getAccountInfo(getFairLaunchConfigAddress(mint, programId), commitment);
  if (account === null || !account.owner.equals(programId)) return null;
  return decodeFairLaunchConfig(account.data);
}

/** Read a mint's slot counter; `null` before the first buy created it. */
export async function readFairLaunchCounter(
  connection: Pick<Connection, 'getAccountInfo'>,
  mint: PublicKey,
  programId: PublicKey,
  commitment: Commitment = 'confirmed'
): Promise<FairLaunchCounter | null> {
  const account = await connection.getAccountInfo(getFairLaunchCounterAddress(mint, programId), commitment);
  if (account === null || !account.owner.equals(programId)) return null;
  return decodeFairLaunchCounter(account.data);
}
