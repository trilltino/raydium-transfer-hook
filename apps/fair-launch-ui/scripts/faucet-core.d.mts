import type { Connection, Keypair, PublicKey } from '@solana/web3.js';

export const repoRoot: string;
export const MAX_TOKENS: bigint;
export const DEFAULT_TOKENS: bigint;
export interface FaucetConfig {
  keypairPath: string;
  rpcUrl: string;
}
export interface FundResult {
  mint: string;
  status: 'minted' | 'skipped';
  reason?: string;
  account?: string;
  amount?: string;
  decimals?: number;
}
export function readKeypair(path: string): Keypair;
export function faucetFor(cluster: string, env: Record<string, string | undefined>): FaucetConfig | null;
export function connectionFor(config: FaucetConfig): Connection;
export function readPoolMints(connection: Connection, pool: string, programs: { cpmm: string; clmm: string }): Promise<PublicKey[]>;
export function fundWallet(args: {
  connection: Connection;
  authority: Keypair;
  wallet: string;
  mints: string[];
  tokens?: bigint;
}): Promise<{ signature: string | null; results: FundResult[] }>;
