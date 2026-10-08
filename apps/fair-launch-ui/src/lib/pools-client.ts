import type { HookEnvironment } from '@raydium-transfer-hook/client';

export const POOL_HOOKS = [
  { value: 'fair-launch', label: 'Fair Launch (anti-bundle, anti-snipe limits)' },
  { value: 'creator-commitment', label: 'Creator Commitment (vesting floor)' },
  { value: 'holder-rewards', label: 'Holder Rewards (balance × time)' },
] as const;
export const POOL_AMMS = [
  { value: 'cpmm', label: 'CPMM' },
  { value: 'clmm', label: 'CLMM' },
] as const;
export type PoolHook = (typeof POOL_HOOKS)[number]['value'];
export type PoolAmm = (typeof POOL_AMMS)[number]['value'];

export interface PoolRun {
  status: 'running' | 'done' | 'failed';
  error?: string;
  pool?: string;
  hookedMint?: string;
  quoteMint?: string;
  hook: string;
  amm: string;
  /** The last lines the pool command printed. */
  log: string[];
}

/** Whether the dev server can create a pool for this environment (it needs the pool admin's key; a static build never can). */
export async function poolCreationAvailable(environment: HookEnvironment, fetchFn: typeof fetch = fetch): Promise<boolean> {
  try {
    const response = await fetchFn('/pools');
    if (!response.ok) return false;
    return ((await response.json()) as Record<string, boolean>)[environment.cluster] === true;
  } catch {
    return false;
  }
}

/** Ask for a new demo pool; returns the run to follow. */
export async function startPool(
  environment: HookEnvironment,
  request: { hook: PoolHook; amm: PoolAmm; wallet: string },
  fetchFn: typeof fetch = fetch
): Promise<string> {
  const response = await fetchFn('/pools', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ cluster: environment.cluster, ...request }),
  });
  const payload = (await response.json().catch(() => ({}))) as { id?: string; error?: string };
  if (!response.ok || !payload.id) throw new Error(payload.error ?? `the pool endpoint answered ${response.status}`);
  return payload.id;
}

export async function poolRun(id: string, fetchFn: typeof fetch = fetch): Promise<PoolRun> {
  const response = await fetchFn(`/pools/${id}`);
  if (!response.ok) throw new Error(`the pool run is not known (${response.status})`);
  return (await response.json()) as PoolRun;
}

/** Follow a run until it finishes, reporting each state. Resolves with the final one. */
export async function followPool(
  id: string,
  onUpdate: (run: PoolRun) => void,
  options: { fetchFn?: typeof fetch; wait?: (ms: number) => Promise<void>; every?: number; limit?: number } = {}
): Promise<PoolRun> {
  const wait = options.wait ?? ((ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms)));
  const limit = options.limit ?? 600;
  for (let i = 0; i < limit; i += 1) {
    const run = await poolRun(id, options.fetchFn);
    onUpdate(run);
    if (run.status !== 'running') return run;
    await wait(options.every ?? 2000);
  }
  throw new Error('the pool is taking too long; check the dev server’s terminal');
}
