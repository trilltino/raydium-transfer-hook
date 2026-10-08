import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { Connection, type ParsedTransactionWithMeta } from '@solana/web3.js';
import { type TraceView, buildTrace } from './trace.ts';

/**
 * Set by `vite.config.ts` when `TRITON_DEVNET_RPC_URL` is in `apps/fair-launch-ui/.env.local`. The URL
 * itself is never given to the page: Vite forwards `/triton/devnet` to it on the server side.
 */
declare const __TRITON_DEVNET_PROXY__: boolean;
export const TRITON_PROXY_PATH = '/triton/devnet';

export interface TraceSource {
  /** Where the transaction is read from, shown on the card. */
  label: string;
  fetchTransaction(signature: string): Promise<ParsedTransactionWithMeta | null>;
}

export interface SourceOptions {
  /** Whether the Triton proxy exists (default: whether the build was given a Triton endpoint). */
  triton?: boolean;
  origin?: string;
  fetch?: typeof fetch;
}

/**
 * Where to read a landed transaction from. Devnet goes through our Triton One endpoint when the dev server
 * has one; otherwise (and for a local validator) the environment's own RPC answers.
 */
export function traceSourceFor(environment: HookEnvironment, options: SourceOptions = {}): TraceSource {
  const triton = options.triton ?? (typeof __TRITON_DEVNET_PROXY__ !== 'undefined' && __TRITON_DEVNET_PROXY__);
  const useTriton = environment.cluster === 'devnet' && triton;
  const origin = options.origin ?? (typeof window === 'undefined' ? 'http://127.0.0.1:5173' : window.location.origin);
  const url = useTriton ? `${origin}${TRITON_PROXY_PATH}` : environment.rpcUrl;
  const label = useTriton ? 'Triton One (devnet)' : environment.cluster === 'devnet' ? 'public devnet RPC' : 'local validator';
  const connection = new Connection(url, { commitment: 'confirmed', ...(options.fetch ? { fetch: options.fetch } : {}) });
  return {
    label,
    fetchTransaction: (signature) => connection.getParsedTransaction(signature, { commitment: 'confirmed', maxSupportedTransactionVersion: 0 }),
  };
}

export type TraceResult =
  | { status: 'ready'; trace: TraceView }
  /** The RPC answered, but has not (yet) indexed the signature. */
  | { status: 'not-observed'; message: string }
  | { status: 'unavailable'; message: string };

export interface TraceRequest {
  environment: HookEnvironment;
  signature: string;
  source?: TraceSource;
  /** How long to wait between tries while the RPC catches up with a fresh transaction. */
  wait?: (milliseconds: number) => Promise<void>;
  attempts?: number;
  onAttempt?: (attempt: number, of: number) => void;
}

const delay = (milliseconds: number) => new Promise<void>((resolve) => setTimeout(resolve, milliseconds));

/**
 * Read a landed transaction and turn it into steps. A fresh signature can take a few seconds to reach the
 * RPC, so an empty answer is retried before it is reported.
 */
export async function traceTransaction(request: TraceRequest): Promise<TraceResult> {
  const { environment, signature } = request;
  const source = request.source ?? traceSourceFor(environment);
  const wait = request.wait ?? delay;
  const attempts = request.attempts ?? 6;
  try {
    for (let attempt = 1; attempt <= attempts; attempt += 1) {
      request.onAttempt?.(attempt, attempts);
      const transaction = await source.fetchTransaction(signature);
      if (transaction) return { status: 'ready', trace: buildTrace(transaction, environment, source.label) };
      if (attempt < attempts) await wait(environment.cluster === 'devnet' ? 2000 : 500);
    }
    return { status: 'not-observed', message: `${source.label} has not returned this transaction yet. The Solscan link works once it is indexed.` };
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    return { status: 'unavailable', message: `Could not read the transaction from ${source.label} (${reason}).` };
  }
}
