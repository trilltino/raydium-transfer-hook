import type { HookEnvironment } from '@raydium-transfer-hook/client';
import type { PublicKey } from '@solana/web3.js';
import { clmmAdapter } from './clmm.ts';
import { cpmmAdapter } from './cpmm.ts';
import type { HookAwareSwapAdapter } from './types.ts';

export { clmmAdapter, cpmmAdapter };
export type { AdapterContext, HookAwareSwapAdapter } from './types.ts';

/** The adapter for a pool account's owner program, or null if the environment has no such AMM. */
export function adapterForOwner(environment: HookEnvironment, owner: PublicKey): HookAwareSwapAdapter | null {
  return [cpmmAdapter, clmmAdapter].find((adapter) => adapter.programId(environment).equals(owner)) ?? null;
}

export function adapterForKind(kind: 'cpmm' | 'clmm'): HookAwareSwapAdapter {
  return kind === 'cpmm' ? cpmmAdapter : clmmAdapter;
}
