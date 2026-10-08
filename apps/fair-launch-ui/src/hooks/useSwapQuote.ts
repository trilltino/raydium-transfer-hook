import { useMemo } from 'react';
import type { HookAwareSwapAdapter } from '../adapters/index.ts';
import type { PoolView } from '../lib/pool.ts';
import type { SwapQuote } from '../lib/quote.ts';

/** The exact-input quote for the typed amount, from Raydium's curve math on freshly loaded reserves. */
export function useSwapQuote(
  adapter: HookAwareSwapAdapter | null,
  pool: PoolView | null,
  inputIsA: boolean,
  amountIn: bigint | null,
  slippageBps: number
): SwapQuote | null {
  return useMemo(() => {
    if (!adapter || !pool || amountIn === null || amountIn <= 0n) return null;
    try {
      return adapter.quote(pool, inputIsA, amountIn, slippageBps);
    } catch (error) {
      // Not enough liquidity, or the pool cannot be quoted: the form then shows no quote.
      console.debug('no quote', error);
      return null;
    }
  }, [adapter, pool, inputIsA, amountIn, slippageBps]);
}
