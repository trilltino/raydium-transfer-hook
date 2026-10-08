import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { useEffect, useState } from 'react';
import { POOL_AMMS, POOL_HOOKS, type PoolAmm, type PoolHook, type PoolRun, followPool, poolCreationAvailable, startPool } from '../lib/pools-client.ts';

export interface CreateDemoPoolProps {
  environment: HookEnvironment;
  /** The connected wallet, which receives test tokens; without one the pool is made for the pool admin and tokens come from "Get test tokens". */
  wallet: string | null;
  /** The pool admin's own address, used as the recipient when no wallet is connected. */
  fallbackWallet?: string;
  onOpen: (pool: string) => void;
}

/** The pool admin on the integration devnet and the local fixture: tokens go here when no wallet is connected. */
const ADMIN_FALLBACK = 'QHgnAZswA5wt8ABUv5n7yM4FXFJdNwLsNYXKSVKB1Pm';

/**
 * "Create a demo pool": the dev server runs this repository's pool command with the pool admin's key and makes a new
 * hooked token and a real pool, then the page opens it. It is a demo and development tool, so it only appears where
 * the dev server has that key.
 */
export function CreateDemoPool({ environment, wallet, fallbackWallet = ADMIN_FALLBACK, onOpen }: CreateDemoPoolProps) {
  const [available, setAvailable] = useState<boolean | null>(null);
  const [open, setOpen] = useState(false);
  const [hook, setHook] = useState<PoolHook>('fair-launch');
  const [amm, setAmm] = useState<PoolAmm>('cpmm');
  const [run, setRun] = useState<PoolRun | null>(null);
  const [error, setError] = useState<string | null>(null);
  const busy = run?.status === 'running';

  useEffect(() => {
    let cancelled = false;
    void poolCreationAvailable(environment).then((ok) => !cancelled && setAvailable(ok));
    return () => {
      cancelled = true;
    };
  }, [environment]);

  const create = async () => {
    setError(null);
    setRun({ status: 'running', hook, amm, log: ['Starting…'] });
    try {
      const id = await startPool(environment, { hook, amm, wallet: wallet ?? fallbackWallet });
      const final = await followPool(id, setRun);
      if (final.status === 'done' && final.pool) onOpen(final.pool);
      else setError(final.error ?? 'The pool could not be created.');
    } catch (reason) {
      setRun(null);
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  return (
    <div className="picker-action" data-testid="create-demo-pool">
      <button type="button" className="btn btn-primary" aria-expanded={open} onClick={() => setOpen(!open)}>
        Create a demo pool
      </button>
      {open && (
        <div className="picker-panel">
          {available === false ? (
            <p className="muted" data-testid="create-pool-unavailable">
              This needs the dev server on a machine that holds the pool admin’s key{environment.cluster === 'devnet' ? ' (FAUCET_KEYPAIR in apps/fair-launch-ui/.env.local)' : ' and a running local validator'}.
              Creating a pool for a hooked token needs the admin’s approval of that token, which a browser wallet cannot give.
            </p>
          ) : (
            <>
              <p className="muted small">
                Makes a new hooked token and a real {environment.cluster === 'devnet' ? 'devnet' : 'local'} pool of our forked Raydium, then opens it.{' '}
                {wallet ? 'Your connected wallet receives test tokens.' : 'No wallet is connected, so use “Get test tokens” afterwards.'}
              </p>
              <div className="picker-fields">
                <label>
                  <span className="visually-hidden">Hook</span>
                  <select className="select" value={hook} disabled={busy} onChange={(event) => setHook(event.target.value as PoolHook)}>
                    {POOL_HOOKS.map((entry) => (
                      <option key={entry.value} value={entry.value}>
                        {entry.label}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  <span className="visually-hidden">AMM</span>
                  <select className="select" value={amm} disabled={busy} onChange={(event) => setAmm(event.target.value as PoolAmm)}>
                    {POOL_AMMS.map((entry) => (
                      <option key={entry.value} value={entry.value}>
                        {entry.label}
                      </option>
                    ))}
                  </select>
                </label>
                <button type="button" className="btn btn-primary" disabled={available !== true || busy} onClick={() => void create()}>
                  {busy ? 'Creating…' : 'Create pool'}
                </button>
              </div>
            </>
          )}
          {run?.status === 'running' && (
            <div className="picker-log" role="status" data-testid="create-pool-progress">
              <p className="muted small">Creating the token and the pool takes a minute or two.</p>
              <pre className="mono small">{run.log.join('\n')}</pre>
            </div>
          )}
          {error && (
            <p className="notice notice-error" role="alert" data-testid="create-pool-error">
              {error}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
