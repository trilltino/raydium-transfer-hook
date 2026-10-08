import { HookClientError } from '../hook/errors.ts';

const BPS = 10_000n;

function checkBps(slippageBps: number): bigint {
  if (!Number.isInteger(slippageBps) || slippageBps < 0 || slippageBps > 10_000) {
    throw new HookClientError('bad-instruction-input', `slippage ${slippageBps} bps is outside 0..10000`);
  }
  return BigInt(slippageBps);
}

/** The least the trader accepts for an exact-input swap whose quote promises `expectedOut`. Rounds down. */
export function minimumAmountOut(expectedOut: bigint, slippageBps: number): bigint {
  return (expectedOut * (BPS - checkBps(slippageBps))) / BPS;
}

/** The most the trader will spend for an exact-output swap whose quote needs `expectedIn`. Rounds up. */
export function maximumAmountIn(expectedIn: bigint, slippageBps: number): bigint {
  return (expectedIn * (BPS + checkBps(slippageBps)) + BPS - 1n) / BPS;
}

/** Price impact in basis points of the spot price: how far the average price is from the pool's spot price. */
export function priceImpactBps(amountIn: bigint, amountOut: bigint, reserveIn: bigint, reserveOut: bigint): number {
  if (amountIn <= 0n || reserveIn <= 0n || reserveOut <= 0n) return 0;
  // spot out for amountIn at the current price vs what the swap pays
  const spotOut = (amountIn * reserveOut) / reserveIn;
  if (spotOut === 0n) return 0;
  const lost = spotOut > amountOut ? spotOut - amountOut : 0n;
  return Number((lost * BPS) / spotOut);
}
