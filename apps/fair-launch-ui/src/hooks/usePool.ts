import type { Raydium } from '@raydium-io/raydium-sdk-v2';
import type { HookEnvironment } from '@raydium-transfer-hook/client';
import type { Connection, PublicKey } from '@solana/web3.js';
import { useCallback, useEffect, useState } from 'react';
import { type PoolContext, PoolRefused, loadPoolContext } from '../lib/chain.ts';
import { checkPool } from '../lib/pool.ts';

export type PoolState =
  | { status: 'none' }
  | { status: 'loading' }
  | { status: 'error'; message: string }
  | { status: 'blocked'; context: PoolContext | null; reason: string }
  | { status: 'ready'; context: PoolContext };

/** Load a pool and the Fair Launch state around it, and fail closed if the pool is not the expected one. */
export function usePool(
  connection: Connection,
  raydium: Raydium | null,
  environment: HookEnvironment,
  poolId: PublicKey | null
) {
  const [state, setState] = useState<PoolState>({ status: poolId ? 'loading' : 'none' });
  const key = poolId?.toBase58() ?? '';

  const load = useCallback(async (): Promise<PoolContext> => {
    if (!raydium || !poolId) throw new Error('no pool is selected');
    return loadPoolContext(connection, raydium, environment, poolId);
  }, [connection, raydium, environment, poolId]);

  const refresh = useCallback(async () => {
    if (!poolId) {
      setState({ status: 'none' });
      return;
    }
    if (!raydium) return;
    try {
      const context = await load();
      const check = checkPool(
        context.pool,
        context.adapter.programId(environment),
        context.launch ? { config: context.launch.config, hookedSide: context.launch.hookedSide } : null
      );
      setState(check.ok ? { status: 'ready', context } : { status: 'blocked', context, reason: check.reason });
    } catch (cause) {
      if (cause instanceof PoolRefused) {
        setState({ status: 'blocked', context: null, reason: cause.message });
        return;
      }
      setState({ status: 'error', message: cause instanceof Error ? cause.message : String(cause) });
    }
  }, [environment, load, poolId, raydium]);

  useEffect(() => {
    setState({ status: poolId ? 'loading' : 'none' });
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, raydium]);

  return { state, refresh, load };
}
