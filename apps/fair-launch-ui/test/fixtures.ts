import type { CreatorCommitmentConfig, FairLaunchConfig, FairLaunchCounter, HolderRewardsGlobal } from '@raydium-transfer-hook/client';
import { PublicKey } from '@solana/web3.js';
import { cpmmAdapter } from '../src/adapters/index.ts';
import type { PoolContext } from '../src/lib/chain.ts';
import type { PoolView } from '../src/lib/pool.ts';

export const key = (byte: number): PublicKey => new PublicKey(Buffer.alloc(32, byte));

export const FAIR_LAUNCH_PROGRAM = key(0xf1);
export const CPMM_PROGRAM = key(0xc1);

export function poolView(overrides: Partial<PoolView> = {}): PoolView {
  return {
    kind: 'cpmm',
    poolId: key(1),
    programId: CPMM_PROGRAM,
    authority: key(2),
    ammConfig: key(3),
    observation: key(4),
    tokenA: { mint: key(0x11), tokenProgram: key(0x21), decimals: 6, vault: key(0x31), reserve: 1_000_000_000_000n },
    tokenB: { mint: key(0x12), tokenProgram: key(0x22), decimals: 6, vault: key(0x32), reserve: 1_000_000_000_000n },
    status: 0,
    cpmm: {
      reserveA: 1_000_000_000_000n,
      reserveB: 1_000_000_000_000n,
      tradeFeeRate: 2_500n,
      creatorFeeRate: 0n,
      protocolFeeRate: 120_000n,
      fundFeeRate: 40_000n,
      feeOn: 0,
    },
    clmm: null,
    ...overrides,
  };
}

export function launchConfig(overrides: Partial<FairLaunchConfig> = {}): FairLaunchConfig {
  const now = BigInt(Math.floor(Date.now() / 1000));
  return {
    bump: 255,
    mint: key(0x11),
    venues: [key(0x31)],
    windowStart: now - 600n,
    windowEnd: now + 1_800n,
    maxBuy: 10_000_000_000n,
    maxWallet: 50_000_000_000n,
    maxBuysPerSlot: 3,
    maxPriorityMicroLamports: 1_000n,
    ...overrides,
  };
}

export interface ContextOptions {
  config?: Partial<FairLaunchConfig> | null;
  counter?: FairLaunchCounter | null;
  slot?: bigint;
  /** Make the pool's hooked token a creator-commitment token instead of a fair-launch one. */
  commitment?: CreatorCommitmentConfig;
  /** Make the pool's hooked token a holder-rewards token instead of a fair-launch one. */
  rewards?: HolderRewardsGlobal;
}

export const CREATOR_PROGRAM = key(0xf2);
export const REWARDS_PROGRAM = key(0xf3);

/** A pool with the hooked token on the A side, vault `key(0x31)` a launch venue (when it is a launch). */
export function poolContext({ config = {}, counter = null, slot = 100n, commitment, rewards }: ContextOptions = {}): PoolContext {
  const pool = poolView();
  const hookProgram = commitment ? CREATOR_PROGRAM : rewards ? REWARDS_PROGRAM : FAIR_LAUNCH_PROGRAM;
  const hooked = { mint: pool.tokenA.mint, tokenProgram: pool.tokenA.tokenProgram, hookProgramId: hookProgram, hookAuthority: null, decimals: 6 };
  const plain = { mint: pool.tokenB.mint, tokenProgram: pool.tokenB.tokenProgram, hookProgramId: null, hookAuthority: null, decimals: 6 };
  const other = Boolean(commitment || rewards);
  return {
    adapter: cpmmAdapter,
    pool,
    hookA: hooked,
    hookB: plain,
    launch: other || config === null ? null : { config: launchConfig(config), counter, hookedSide: 'A' },
    commitment: commitment ? { config: commitment, hookedSide: 'A' } : null,
    rewards: rewards ? { global: rewards, hookedSide: 'A', rewardMint: { mint: key(0x41), tokenProgram: key(0x22), decimals: 6 } } : null,
    slot,
  };
}
