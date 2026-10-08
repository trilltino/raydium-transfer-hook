import { CurveCalculator } from '@raydium-io/raydium-sdk-v2';
import BN from 'bn.js';
import type { PublicKey } from '@solana/web3.js';
import { minimumAmountOut, priceImpactBps } from '@raydium-transfer-hook/client';

/** The `FeeOn` values of Raydium CPMM pools: which token carries the creator fee. */
export const FEE_ON_BOTH = 0;
export const FEE_ON_ONLY_A = 1;
export const FEE_ON_ONLY_B = 2;

export interface PoolEconomics {
  /** Reserves net of owed protocol, fund and creator fees, as the program uses them. */
  reserveA: bigint;
  reserveB: bigint;
  tradeFeeRate: bigint;
  creatorFeeRate: bigint;
  protocolFeeRate: bigint;
  fundFeeRate: bigint;
  feeOn: number;
}

export interface SwapQuote {
  amountIn: bigint;
  amountOut: bigint;
  minimumOut: bigint;
  tradeFee: bigint;
  creatorFee: bigint;
  priceImpactBps: number;
  /** Output per input, in base units. */
  executionPrice: number;
  /** CLMM only: the tick arrays the swap walks, in order (empty for CPMM). */
  tickArrays: PublicKey[];
  /** CLMM only: the tick-array bitmap extension, if the swap needs it. */
  bitmapExtension: PublicKey | null;
}

const bn = (value: bigint): BN => new BN(value.toString());
const big = (value: BN): bigint => BigInt(value.toString());

/**
 * An exact-input CPMM quote from Raydium's own `CurveCalculator` (the SDK owns the math, so fee rules
 * stay in step with Raydium). `inputIsA` says which pool token is being sold.
 */
export function quoteBaseInput(pool: PoolEconomics, amountIn: bigint, inputIsA: boolean, slippageBps: number): SwapQuote {
  const [reserveIn, reserveOut] = inputIsA ? [pool.reserveA, pool.reserveB] : [pool.reserveB, pool.reserveA];
  // Which side carries the creator fee follows the program (`PoolState::is_creator_fee_on_input`):
  // both tokens, or only the token being sold. The SDK helper `computeSwapAmount` ignores the
  // direction, so this is decided here and the SDK only does the curve arithmetic.
  const creatorFeeOnInput = pool.feeOn === FEE_ON_BOTH || (inputIsA ? pool.feeOn === FEE_ON_ONLY_A : pool.feeOn === FEE_ON_ONLY_B);
  const result = CurveCalculator.swapBaseInput(
    bn(amountIn),
    bn(reserveIn),
    bn(reserveOut),
    bn(pool.tradeFeeRate),
    bn(pool.creatorFeeRate),
    bn(pool.protocolFeeRate),
    bn(pool.fundFeeRate),
    creatorFeeOnInput
  );
  const amountOut = big(result.outputAmount);
  return {
    amountIn,
    amountOut,
    minimumOut: minimumAmountOut(amountOut, slippageBps),
    tradeFee: big(result.tradeFee),
    creatorFee: big(result.creatorFee),
    priceImpactBps: priceImpactBps(amountIn, amountOut, reserveIn, reserveOut),
    executionPrice: amountIn === 0n ? 0 : Number(amountOut) / Number(amountIn),
    tickArrays: [],
    bitmapExtension: null,
  };
}
