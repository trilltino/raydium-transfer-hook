import { PoolUtils } from '@raydium-io/raydium-sdk-v2';
import { minimumAmountOut, programKeys } from '@raydium-transfer-hook/client';
import { PublicKey } from '@solana/web3.js';
import BN from 'bn.js';
import type { PoolView } from '../lib/pool.ts';
import type { SwapQuote } from '../lib/quote.ts';
import { prepareSwap } from '../lib/swap.ts';
import type { AdapterContext, HookAwareSwapAdapter } from './types.ts';

const big = (value: { toString(): string }): bigint => BigInt(value.toString());

export const clmmAdapter: HookAwareSwapAdapter = {
  kind: 'clmm',
  instruction: 'swap_v3',
  programId: (environment) => programKeys(environment).clmm,

  /** Read a CLMM pool, its tick arrays and its bitmap through the SDK; the program is the account's owner. */
  async loadPool({ raydium, connection }: AdapterContext, poolId: PublicKey): Promise<PoolView> {
    const key = poolId.toBase58();
    const info = await raydium.clmm.getPoolInfoFromRpc(key);
    const pool = info.computePoolInfo;
    const [epochInfo, slot] = await Promise.all([connection.getEpochInfo('confirmed'), connection.getSlot('confirmed')]);
    const blockTime = await connection.getBlockTime(slot);
    return {
      kind: 'clmm',
      poolId,
      programId: pool.programId,
      // The pool-state account signs the output leg.
      authority: poolId,
      ammConfig: new PublicKey(pool.ammConfig.id),
      observation: pool.observationId,
      tokenA: {
        mint: new PublicKey(pool.mintA.address),
        tokenProgram: new PublicKey(pool.mintA.programId),
        decimals: pool.mintA.decimals,
        vault: pool.vaultA,
        reserve: BigInt(Math.trunc(info.poolInfo.mintAmountA)),
      },
      tokenB: {
        mint: new PublicKey(pool.mintB.address),
        tokenProgram: new PublicKey(pool.mintB.programId),
        decimals: pool.mintB.decimals,
        vault: pool.vaultB,
        reserve: BigInt(Math.trunc(info.poolInfo.mintAmountB)),
      },
      status: Number(info.rpcPoolInfo.status),
      cpmm: null,
      clmm: {
        computePoolInfo: pool,
        tickArrayCache: info.tickData[key],
        epochInfo,
        blockTimestamp: blockTime ?? Math.floor(Date.now() / 1000),
        bitmapExtension: pool.exBitmapAccount,
      },
    };
  },

  quote(pool, inputIsA, amountIn, slippageBps): SwapQuote {
    const data = pool.clmm;
    if (!data) throw new Error('not a CLMM pool');
    const tokenIn = inputIsA ? pool.tokenA : pool.tokenB;
    const out = PoolUtils.computeAmountOut({
      poolInfo: data.computePoolInfo,
      tickarrayBitmapExtension: data.computePoolInfo.exBitmapInfo,
      tickArrayCache: data.tickArrayCache,
      baseMint: tokenIn.mint,
      amountIn: new BN(amountIn.toString()),
      slippage: slippageBps / 10_000,
      epochInfo: data.epochInfo,
      catchLiquidityInsufficient: false,
      blockTimestamp: data.blockTimestamp,
    });
    const amountOut = big(out.amountOut.amount);
    // The program takes the bitmap extension after the tick arrays; the SDK lists it first.
    const bitmap = data.bitmapExtension;
    const tickArrays = out.remainingAccounts.filter((key) => !key.equals(bitmap));
    const usesBitmap = out.remainingAccounts.some((key) => key.equals(bitmap));
    return {
      amountIn,
      amountOut,
      minimumOut: minimumAmountOut(amountOut, slippageBps),
      tradeFee: big(out.fee),
      creatorFee: 0n,
      priceImpactBps: Math.round(Number(out.priceImpact.toFixed(6)) * 100),
      executionPrice: amountIn === 0n ? 0 : Number(amountOut) / Number(amountIn),
      tickArrays,
      bitmapExtension: usesBitmap ? bitmap : null,
    };
  },

  buildSwap: prepareSwap,
};
