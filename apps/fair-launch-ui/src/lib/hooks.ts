import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { PublicKey } from '@solana/web3.js';
import type { PoolContext } from './chain.ts';

/** The example hooks this page has a policy panel for. */
export type KnownHook = 'fair-launch' | 'creator-commitment' | 'holder-rewards';

/** The hooked token of a pool whose hook is one of the examples, with the side of the pool it is on. */
export interface HookedToken {
  side: 'A' | 'B';
  mint: PublicKey;
  kind: KnownHook;
}

/** The first token of the pool that has one of the example hooks set up (an unrecognised hook has no panel). */
export function hookedToken(context: Pick<PoolContext, 'pool' | 'launch' | 'commitment' | 'rewards'>): HookedToken | null {
  const found: [KnownHook, 'A' | 'B'] | null = context.launch
    ? ['fair-launch', context.launch.hookedSide]
    : context.commitment
      ? ['creator-commitment', context.commitment.hookedSide]
      : context.rewards
        ? ['holder-rewards', context.rewards.hookedSide]
        : null;
  if (!found) return null;
  const [kind, side] = found;
  return { side, kind, mint: side === 'A' ? context.pool.tokenA.mint : context.pool.tokenB.mint };
}

export const HOOK_TITLES: Record<KnownHook, string> = {
  'fair-launch': 'Fair Launch',
  'creator-commitment': 'Creator Commitment',
  'holder-rewards': 'Holder Rewards',
};

/** The program id of an example hook in `environment`; throws if the environment does not list it. */
export function hookProgramId(environment: HookEnvironment, kind: KnownHook): PublicKey {
  const text =
    kind === 'fair-launch'
      ? environment.fairLaunchProgramId
      : kind === 'creator-commitment'
        ? environment.creatorCommitmentProgramId
        : environment.holderRewardsProgramId;
  if (!text) throw new Error(`the environment ${environment.name} has no ${HOOK_TITLES[kind]} program`);
  return new PublicKey(text);
}

/** A name for the hook on a pool, for the "Hook" row of the swap summary. */
export function hookSummary(known: KnownHook | null, hasUnknownHook: boolean, detail: string): string {
  if (known) return `${HOOK_TITLES[known]}${detail ? ` · ${detail}` : ''}`;
  return hasUnknownHook ? 'Unrecognised hook' : 'None';
}
