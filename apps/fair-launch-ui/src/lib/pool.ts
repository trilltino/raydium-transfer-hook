import type { FairLaunchConfig, TransferHookInfo } from '@raydium-transfer-hook/client';
import type { ComputeClmmPoolInfo, PoolUtils } from '@raydium-io/raydium-sdk-v2';
import { type EpochInfo, PublicKey } from '@solana/web3.js';
import type { PoolEconomics } from './quote.ts';

/** One side of a pool, as the swap needs it. */
export interface PoolToken {
  mint: PublicKey;
  tokenProgram: PublicKey;
  decimals: number;
  vault: PublicKey;
  /** Reserve net of owed fees, in base units. */
  reserve: bigint;
}

/** CLMM data the quote needs, loaded with the pool so the numbers agree. */
export interface ClmmPoolData {
  computePoolInfo: ComputeClmmPoolInfo;
  tickArrayCache: Parameters<typeof PoolUtils.computeAmountOut>[0]['tickArrayCache'];
  epochInfo: EpochInfo;
  blockTimestamp: number;
  /** The pool's tick-array bitmap extension account, if a swap needs it. */
  bitmapExtension: PublicKey;
}

export interface PoolView {
  kind: 'cpmm' | 'clmm';
  poolId: PublicKey;
  /** The program that owns the pool account. */
  programId: PublicKey;
  /** The account that signs the output leg: CPMM's pool authority, CLMM's pool-state account. */
  authority: PublicKey;
  ammConfig: PublicKey;
  observation: PublicKey;
  tokenA: PoolToken;
  tokenB: PoolToken;
  /** Pool status bits; bit 2 (value 4) disables swaps (CPMM). */
  status: number;
  /** Fee rates and reserves for CPMM quotes; null for CLMM. */
  cpmm: PoolEconomics | null;
  /** Tick data for CLMM quotes; null for CPMM. */
  clmm: ClmmPoolData | null;
}

export type PoolCheck = { ok: true } | { ok: false; reason: string };

/** Parse the `?pool=` query value. */
export function parsePoolParam(value: string | null): PublicKey | { error: string } | null {
  if (value === null || value === '') return null;
  try {
    return new PublicKey(value);
  } catch {
    return { error: `“${value}” is not a valid pool address.` };
  }
}

/**
 * Fail closed unless the pool is the expected integration pool: owned by the environment's Raydium
 * program, open for swaps, and (when a fair-launch config exists for one of its mints) holding the
 * configured vault.
 */
export function checkPool(
  pool: PoolView,
  expectedProgram: PublicKey,
  launch: { config: FairLaunchConfig; hookedSide: 'A' | 'B' } | null
): PoolCheck {
  if (!pool.programId.equals(expectedProgram)) {
    return {
      ok: false,
      reason: `This pool belongs to ${pool.programId.toBase58()}, not this environment's Raydium program ${expectedProgram.toBase58()}.`,
    };
  }
  if ((pool.status & 4) !== 0) return { ok: false, reason: 'Swaps are disabled on this pool.' };
  if (launch) {
    const token = launch.hookedSide === 'A' ? pool.tokenA : pool.tokenB;
    if (!launch.config.mint.equals(token.mint)) {
      return { ok: false, reason: 'The Fair Launch configuration is for a different mint than this pool’s hooked token.' };
    }
    if (!launch.config.venues.some((venue) => venue.equals(token.vault))) {
      return {
        ok: false,
        reason: 'This pool’s vault is not one of the Fair Launch venues, so the launch rules would not apply to buys here.',
      };
    }
  }
  return { ok: true };
}

export interface TokenInfo {
  mint: PublicKey;
  hook: TransferHookInfo;
}

/** Which side of the pool, if any, is a fair-launch token of this environment. */
export function fairLaunchSide(a: TokenInfo, b: TokenInfo, fairLaunchProgram: PublicKey): 'A' | 'B' | null {
  if (a.hook.hookProgramId?.equals(fairLaunchProgram)) return 'A';
  if (b.hook.hookProgramId?.equals(fairLaunchProgram)) return 'B';
  return null;
}
