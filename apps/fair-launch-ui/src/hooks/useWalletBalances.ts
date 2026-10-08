import type { Connection, PublicKey } from '@solana/web3.js';
import { useCallback, useEffect, useState } from 'react';
import { loadBalance } from '../lib/chain.ts';
import type { PoolView } from '../lib/pool.ts';

export interface Balances {
  a: bigint;
  b: bigint;
}

export function useWalletBalances(connection: Connection, owner: PublicKey | null, pool: PoolView | null) {
  const [balances, setBalances] = useState<Balances | null>(null);
  const poolKey = pool?.poolId.toBase58() ?? '';
  const ownerKey = owner?.toBase58() ?? '';

  const refresh = useCallback(async () => {
    if (!owner || !pool) {
      setBalances(null);
      return;
    }
    const [a, b] = await Promise.all([
      loadBalance(connection, owner, pool.tokenA.mint, pool.tokenA.tokenProgram),
      loadBalance(connection, owner, pool.tokenB.mint, pool.tokenB.tokenProgram),
    ]);
    setBalances({ a, b });
  }, [connection, owner, pool]);

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [poolKey, ownerKey, connection]);

  return { balances, refresh };
}
