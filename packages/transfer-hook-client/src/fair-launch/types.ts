import type { PublicKey } from '@solana/web3.js';

/** A fair-launch config. A limit of `0` means that check is off; at least one is on. */
export interface FairLaunchConfig {
  bump: number;
  mint: PublicKey;
  /** One to four pool vaults of the hooked token; a transfer out of one is a buy. */
  venues: PublicKey[];
  windowStart: bigint;
  windowEnd: bigint;
  maxBuy: bigint;
  maxWallet: bigint;
  maxBuysPerSlot: number;
  maxPriorityMicroLamports: bigint;
}

export interface FairLaunchCounter {
  bump: number;
  /** The slot of the last buy. */
  slot: bigint;
  /** Buys in that slot. */
  buys: number;
}
