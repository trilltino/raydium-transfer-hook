import { getCpmmPoolAuthority, programKeys } from '@raydium-transfer-hook/client';
import type { PublicKey } from '@solana/web3.js';
import type { PoolView } from '../lib/pool.ts';
import { quoteBaseInput } from '../lib/quote.ts';
import { prepareSwap } from '../lib/swap.ts';
import type { AdapterContext, HookAwareSwapAdapter } from './types.ts';

const big = (value: { toString(): string }): bigint => BigInt(value.toString());

export const cpmmAdapter: HookAwareSwapAdapter = {
  kind: 'cpmm',
  instruction: 'swap_base_input_v2',
  programId: (environment) => programKeys(environment).cpmm,

  /** Read a CPMM pool through the SDK. The pool's owner (its program) comes from the account, so forks work. */
  async loadPool({ raydium }: AdapterContext, poolId: PublicKey): Promise<PoolView> {
    const rpc = await raydium.cpmm.getRpcPoolInfo(poolId.toBase58(), true);
    const config = rpc.configInfo;
    if (!config) throw new Error('the pool’s AMM config could not be read');
    const reserveA = big(rpc.baseReserve);
    const reserveB = big(rpc.quoteReserve);
    return {
      kind: 'cpmm',
      poolId,
      programId: rpc.programId,
      authority: getCpmmPoolAuthority(rpc.programId),
      ammConfig: rpc.configId,
      observation: rpc.observationId,
      tokenA: { mint: rpc.mintA, tokenProgram: rpc.mintProgramA, decimals: rpc.mintDecimalA, vault: rpc.vaultA, reserve: reserveA },
      tokenB: { mint: rpc.mintB, tokenProgram: rpc.mintProgramB, decimals: rpc.mintDecimalB, vault: rpc.vaultB, reserve: reserveB },
      status: Number(rpc.status),
      cpmm: {
        reserveA,
        reserveB,
        tradeFeeRate: big(config.tradeFeeRate),
        creatorFeeRate: big(config.creatorFeeRate),
        protocolFeeRate: big(config.protocolFeeRate),
        fundFeeRate: big(config.fundFeeRate),
        feeOn: Number(rpc.feeOn),
      },
      clmm: null,
    };
  },

  quote(pool, inputIsA, amountIn, slippageBps) {
    if (!pool.cpmm) throw new Error('not a CPMM pool');
    return quoteBaseInput(pool.cpmm, amountIn, inputIsA, slippageBps);
  },

  buildSwap: prepareSwap,
};
