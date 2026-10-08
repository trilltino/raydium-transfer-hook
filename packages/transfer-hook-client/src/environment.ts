import { PublicKey } from '@solana/web3.js';
import { HookClientError } from './hook/errors.ts';

export type HookCluster = 'localnet' | 'devnet';

/** What the browser needs to know about one deployment of the hook-aware Raydium programs. */
export interface HookEnvironment {
  name: string;
  cluster: HookCluster;
  rpcUrl: string;
  cpmmProgramId: string;
  clmmProgramId: string;
  fairLaunchProgramId: string;
}

/**
 * Read one of the repository's environment files (`environments/localnet.json`,
 * `environments/devnet.json`), as parsed JSON or as text. Only the fields the client needs are kept,
 * and every program id is checked to be a public key.
 */
export function loadEnvironment(source: string | unknown): HookEnvironment {
  const raw = (typeof source === 'string' ? JSON.parse(source) : source) as Record<string, any> | null;
  const need = (value: unknown, what: string): string => {
    if (typeof value !== 'string' || !value) throw new HookClientError('environment', `the environment has no ${what}`);
    return value;
  };
  const key = (value: unknown, what: string): string => {
    const text = need(value, what);
    try {
      return new PublicKey(text).toBase58();
    } catch {
      throw new HookClientError('environment', `the environment's ${what} is not a public key`);
    }
  };
  if (raw === null || typeof raw !== 'object') throw new HookClientError('environment', 'the environment is not an object');
  const cluster = need(raw.cluster, 'cluster');
  if (cluster !== 'localnet' && cluster !== 'devnet') {
    throw new HookClientError('environment', `unsupported cluster \`${cluster}\`: this client supports localnet and devnet only`);
  }
  return {
    name: need(raw.name, 'name'),
    cluster,
    rpcUrl: need(raw.rpc_url ?? raw.rpcUrl, 'rpc_url'),
    cpmmProgramId: key(raw.programs?.cpmm ?? raw.cpmmProgramId, 'programs.cpmm'),
    clmmProgramId: key(raw.programs?.clmm ?? raw.clmmProgramId, 'programs.clmm'),
    fairLaunchProgramId: key(raw.programs?.templates?.fair_launch ?? raw.fairLaunchProgramId, 'programs.templates.fair_launch'),
  };
}

export const programKeys = (env: HookEnvironment) => ({
  cpmm: new PublicKey(env.cpmmProgramId),
  clmm: new PublicKey(env.clmmProgramId),
  fairLaunch: new PublicKey(env.fairLaunchProgramId),
});

/** Shown wherever a program id appears. These are our deployments, not Raydium's. */
export const EXPERIMENTAL_NOTICE = 'Experimental Raydium Transfer Hook environment. Not an official Raydium deployment.';
