import type { Raydium } from '@raydium-io/raydium-sdk-v2';
import type { HookEnvironment } from '@raydium-transfer-hook/client';
import type { Connection, PublicKey } from '@solana/web3.js';
import type { PoolView } from '../lib/pool.ts';
import type { SwapQuote } from '../lib/quote.ts';
import type { PrepareSwapInput, PreparedSwap } from '../lib/swap.ts';

export interface AdapterContext {
  connection: Connection;
  raydium: Raydium;
  environment: HookEnvironment;
}

/**
 * What the React layer needs from an AMM, so the page is the same for CPMM and CLMM: load a pool, quote
 * an exact-input swap, and build the hook-aware swap instruction. The adapter is chosen from the pool
 * account's owner program.
 */
export interface HookAwareSwapAdapter {
  kind: 'cpmm' | 'clmm';
  /** The hook-aware instruction this adapter builds. */
  instruction: 'swap_base_input_v2' | 'swap_v3';
  /** The Raydium program of `environment` this adapter handles. */
  programId(environment: HookEnvironment): PublicKey;
  loadPool(context: AdapterContext, poolId: PublicKey): Promise<PoolView>;
  /** Throws if the pool cannot fill the trade (for example, not enough liquidity). */
  quote(pool: PoolView, inputIsA: boolean, amountIn: bigint, slippageBps: number): SwapQuote;
  buildSwap(input: PrepareSwapInput): Promise<PreparedSwap>;
}
