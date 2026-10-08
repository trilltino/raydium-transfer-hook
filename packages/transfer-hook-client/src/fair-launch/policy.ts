import type { FairLaunchConfig, FairLaunchCounter } from './types.ts';

export type LaunchPhase = 'not-started' | 'active' | 'ended';

export function launchPhase(config: FairLaunchConfig, nowSeconds: bigint): LaunchPhase {
  if (nowSeconds < config.windowStart) return 'not-started';
  if (nowSeconds >= config.windowEnd) return 'ended';
  return 'active';
}

/** A limit that is on, with how much of it this trade would use. */
export interface PolicyRow {
  rule: 'max-buy' | 'max-wallet' | 'buys-per-slot' | 'priority-fee';
  label: string;
  limit: bigint;
  /** The value the trade would reach. */
  used: bigint;
  /** True if the trade would break this rule on chain. */
  violated: boolean;
}

export interface BuyPreview {
  /** Tokens the buy delivers to the trader. */
  amount: bigint;
  /** The trader's balance of the hooked token before the buy. */
  walletBalance: bigint;
  /** The priority fee (micro-lamports per compute unit) the transaction declares, if any. */
  priorityMicroLamports?: bigint;
  currentSlot: bigint;
}

/**
 * What a buy would do against each limit that is on, so the UI can warn before signing. The on-chain
 * hook stays the authority; this only mirrors its arithmetic (`rule::check_buy`). Limits set to 0 are off
 * and produce no row.
 */
export function previewBuy(
  config: FairLaunchConfig,
  counter: FairLaunchCounter | null,
  buy: BuyPreview,
  nowSeconds: bigint
): PolicyRow[] {
  if (launchPhase(config, nowSeconds) !== 'active') return [];
  const rows: PolicyRow[] = [];
  if (config.maxBuy > 0n) {
    rows.push({ rule: 'max-buy', label: 'Buy amount', limit: config.maxBuy, used: buy.amount, violated: buy.amount > config.maxBuy });
  }
  if (config.maxWallet > 0n) {
    const after = buy.walletBalance + buy.amount;
    rows.push({ rule: 'max-wallet', label: 'Wallet after', limit: config.maxWallet, used: after, violated: after > config.maxWallet });
  }
  if (config.maxBuysPerSlot > 0) {
    const inSlot = counter !== null && counter.slot === buy.currentSlot ? counter.buys + 1 : 1;
    rows.push({
      rule: 'buys-per-slot',
      label: 'Buys this slot',
      limit: BigInt(config.maxBuysPerSlot),
      used: BigInt(inSlot),
      violated: inSlot > config.maxBuysPerSlot,
    });
  }
  if (config.maxPriorityMicroLamports > 0n) {
    const declared = buy.priorityMicroLamports ?? 0n;
    rows.push({
      rule: 'priority-fee',
      label: 'Priority fee',
      limit: config.maxPriorityMicroLamports,
      used: declared,
      violated: declared > config.maxPriorityMicroLamports,
    });
  }
  return rows;
}

/** Whether a transfer out of `source` is a buy of this launch (it leaves one of the venues). */
export function isLaunchBuy(config: FairLaunchConfig, source: { equals(other: unknown): boolean }): boolean {
  return config.venues.some((venue) => source.equals(venue));
}
