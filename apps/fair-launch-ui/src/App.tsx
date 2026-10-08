import type { HookEnvironment } from '@raydium-transfer-hook/client';
import type { Connection } from '@solana/web3.js';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useWallet } from '@solana/wallet-adapter-react';
import { readSelection, withParams } from './config.ts';
import { BringYourOwnToken } from './components/BringYourOwnToken.tsx';
import { CreateDemoPool } from './components/CreateDemoPool.tsx';
import { Header } from './components/Header.tsx';
import { SwapCard } from './components/SwapCard.tsx';
import { usePool } from './hooks/usePool.ts';
import { useRaydium } from './hooks/useRaydium.ts';
import { useWalletBalances } from './hooks/useWalletBalances.ts';

export function App() {
  const [search, setSearch] = useState(() => window.location.search);
  const selection = useMemo(() => readSelection(search), [search]);
  const { environment } = selection;
  const poolId = selection.pool && !('error' in selection.pool) ? selection.pool : null;
  const { publicKey } = useWallet();

  const { connection, raydium, error: raydiumError } = useRaydium(environment);
  const { state, refresh, load } = usePool(connection, raydium, environment, poolId);
  const context = state.status === 'ready' || state.status === 'blocked' ? state.context : null;
  const { balances, refresh: refreshBalances } = useWalletBalances(connection, publicKey, context?.pool ?? null);
  const [loadedAt, setLoadedAt] = useState(Date.now());
  useEffect(() => setLoadedAt(Date.now()), [context]);

  const navigate = useCallback((changes: Record<string, string | null>) => {
    const next = withParams(window.location.search, changes);
    window.history.replaceState(null, '', `${window.location.pathname}${next}`);
    setSearch(next);
  }, []);

  const afterSwap = useCallback(() => {
    void refresh();
    void refreshBalances();
  }, [refresh, refreshBalances]);

  return (
    <>
      <Header environment={environment} />
      <main className="page">
        {selection.pool && 'error' in selection.pool && (
          <p className="notice notice-error" role="alert">
            {selection.pool.error}
          </p>
        )}
        {state.status === 'none' && !(selection.pool && 'error' in selection.pool) && (
            <PoolPicker
              environment={environment}
              connection={connection}
              wallet={publicKey?.toBase58() ?? null}
              onOpen={(pool) => navigate({ pool })}
            />
          )}
        {raydiumError && (
          <p className="notice notice-error" role="alert">
            Could not initialise Raydium SDK: {raydiumError}
          </p>
        )}
        {state.status === 'loading' && (
          <p className="notice" role="status" data-testid="loading-pool">
            Loading pool…
          </p>
        )}
        {state.status === 'error' && (
          <p className="notice notice-error" role="alert">
            Could not load this pool: {state.message}
          </p>
        )}
        {state.status === 'blocked' && (
          <p className="notice notice-error" role="alert" data-testid="pool-blocked">
            {state.reason}
          </p>
        )}
        {state.status === 'ready' && (
          <SwapCard
            key={state.context.pool.poolId.toBase58()}
            environment={environment}
            connection={connection}
            context={state.context}
            balances={balances}
            reload={load}
            onSwapped={afterSwap}
            loadedAt={loadedAt}
          />
        )}
      </main>
    </>
  );
}

function PoolPicker({
  environment,
  connection,
  wallet,
  onOpen,
}: {
  environment: HookEnvironment;
  connection: Connection;
  wallet: string | null;
  onOpen: (pool: string) => void;
}) {
  const [value, setValue] = useState('');
  return (
    <section className="card picker" aria-labelledby="picker-title">
      <h2 id="picker-title">Open a pool</h2>
      <p className="muted">
        Paste a CPMM pool address from this environment, or open the page with <span className="mono">?pool=&lt;address&gt;</span>.
      </p>
      <label htmlFor="pool-input" className="visually-hidden">
        Pool address
      </label>
      <input
        id="pool-input"
        className="amount"
        value={value}
        placeholder="Pool address"
        onChange={(event) => setValue(event.target.value)}
        spellCheck={false}
      />
      <button type="button" className="btn btn-primary" disabled={value.trim() === ''} onClick={() => onOpen(value.trim())}>
        Open pool
      </button>
      <div className="picker-actions">
        <CreateDemoPool environment={environment} wallet={wallet} onOpen={onOpen} />
        <BringYourOwnToken environment={environment} connection={connection} />
      </div>
    </section>
  );
}
