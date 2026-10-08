import { EXPERIMENTAL_NOTICE, type HookEnvironment } from '@raydium-transfer-hook/client';
import { ENVIRONMENTS } from './generated/environments.ts';
import { type PoolCheck, parsePoolParam } from './lib/pool.ts';

export { EXPERIMENTAL_NOTICE };

export const DEFAULT_ENVIRONMENT = 'integration-devnet';

export function environmentByName(name: string | null): HookEnvironment {
  const found = ENVIRONMENTS.find((env) => env.name === name || env.cluster === name);
  return found ?? ENVIRONMENTS.find((env) => env.name === DEFAULT_ENVIRONMENT) ?? ENVIRONMENTS[0];
}

export interface Selection {
  environment: HookEnvironment;
  pool: ReturnType<typeof parsePoolParam>;
}

/** Read `?env=` and `?pool=` from a query string. */
export function readSelection(search: string): Selection {
  const params = new URLSearchParams(search);
  return { environment: environmentByName(params.get('env')), pool: parsePoolParam(params.get('pool')) };
}

export function withParams(search: string, changes: Record<string, string | null>): string {
  const params = new URLSearchParams(search);
  for (const [key, value] of Object.entries(changes)) {
    if (value === null) params.delete(key);
    else params.set(key, value);
  }
  const text = params.toString();
  return text ? `?${text}` : '';
}

export const ENVIRONMENT_LIST = ENVIRONMENTS;
export type { PoolCheck };

/** The browser test wallet exists only in builds made with `VITE_E2E=1`; a normal build contains no key handling. */
export const E2E_BUILD = import.meta.env.VITE_E2E === '1';

export const explorerUrl = (environment: HookEnvironment, kind: 'tx' | 'address', value: string): string => {
  const base = `https://explorer.solana.com/${kind}/${value}`;
  return environment.cluster === 'devnet'
    ? `${base}?cluster=devnet`
    : `${base}?cluster=custom&customUrl=${encodeURIComponent(environment.rpcUrl)}`;
};
