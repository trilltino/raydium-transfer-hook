export const HOOKS: string[];
export const AMMS: string[];
export function poolsFor(cluster: string, env: Record<string, string | undefined>): { cluster: string; keypair?: string; feeReceiver?: string } | null;
export interface PoolJobView {
  status: 'running' | 'done' | 'failed';
  error?: string;
  pool?: string;
  hookedMint?: string;
  quoteMint?: string;
  hookProgram?: string;
  hook: string;
  amm: string;
  cluster: string;
  log: string[];
}
export function createJobs(): {
  start(request: { cluster: string; hook: string; amm: string; wallet: string }, env: Record<string, string | undefined>): { id: string } | { error: string };
  view(id: string): PoolJobView | null;
};
