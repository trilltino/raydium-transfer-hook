import type { HookEnvironment } from '@raydium-transfer-hook/client';

export interface FundResult {
  mint: string;
  status: 'minted' | 'skipped';
  reason?: string;
  amount?: string;
}

export interface FaucetAnswer {
  signature: string | null;
  results: FundResult[];
}

/**
 * Whether the dev server can hand out test tokens for this environment. The faucet is a route of the Vite dev
 * server (`/faucet`), which holds the mint authority's keypair; a static build has none, and then this is false.
 */
export async function faucetAvailable(environment: HookEnvironment, fetchFn: typeof fetch = fetch): Promise<boolean> {
  try {
    const response = await fetchFn('/faucet');
    if (!response.ok) return false;
    const payload = (await response.json()) as Record<string, boolean>;
    return payload[environment.cluster] === true;
  } catch {
    return false;
  }
}

/** Ask the faucet to put `tokens` whole tokens of each mint into the wallet. */
export async function requestTokens(
  environment: HookEnvironment,
  wallet: string,
  mints: string[],
  tokens = 100,
  fetchFn: typeof fetch = fetch
): Promise<FaucetAnswer> {
  const response = await fetchFn('/faucet', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ cluster: environment.cluster, wallet, mints, tokens }),
  });
  const payload = (await response.json().catch(() => ({}))) as Partial<FaucetAnswer> & { error?: string };
  if (!response.ok) throw new Error(payload.error ?? `the faucet answered ${response.status}`);
  return { signature: payload.signature ?? null, results: payload.results ?? [] };
}

/** The command that does the same from a terminal, for the hint in Developer details. */
export function fundCommand(environment: HookEnvironment, wallet: string, pool: string): string {
  return `npm --workspace apps/fair-launch-ui run fund -- ${wallet} --pool ${pool} --cluster ${environment.cluster}`;
}
