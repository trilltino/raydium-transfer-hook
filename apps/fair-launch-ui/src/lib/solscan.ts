import type { HookEnvironment } from '@raydium-transfer-hook/client';

export type SolscanKind = 'tx' | 'account' | 'token';

/**
 * A Solscan link for a transaction, an account (wallet, token account, program) or a token mint on the
 * environment's cluster. Devnet has its own Solscan cluster; a local validator is opened through Solscan's
 * custom-RPC mode, which reads the chain from the validator your browser can reach.
 */
export function solscanUrl(environment: HookEnvironment, kind: SolscanKind, value: string): string {
  const base = `https://solscan.io/${kind}/${value}`;
  return environment.cluster === 'devnet'
    ? `${base}?cluster=devnet`
    : `${base}?cluster=custom&customUrl=${encodeURIComponent(environment.rpcUrl)}`;
}
