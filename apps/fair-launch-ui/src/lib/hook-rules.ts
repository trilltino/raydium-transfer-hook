import {
  type CreatorCommitmentConfig,
  type FairLaunchConfig,
  decodeFairLaunchError,
  vestingView,
} from '@raydium-transfer-hook/client';
import { formatAmount } from './amounts.ts';
import type { TraceStep, TraceView } from './trace.ts';

/** The launch rules of the pool's hook, as the page read them from the hook's own config account. */
export type HookPolicy =
  | { kind: 'fair-launch'; config: FairLaunchConfig; decimals: number }
  | { kind: 'creator-commitment'; config: CreatorCommitmentConfig; decimals: number }
  | { kind: 'holder-rewards' };

export interface RuleLine {
  /** What the hook checks, in words. */
  name: string;
  /** The limit, and what this transfer came to, as text. */
  detail: string;
  /** `passed`/`failed` for a check the hook ran, `info` for something it does but cannot refuse over, `skipped` for a rule that did not apply. */
  status: 'passed' | 'failed' | 'info' | 'skipped';
}

export interface Enforcement {
  /** One line naming the hook's rules, shown beside the step. */
  headline: string;
  lines: RuleLine[];
}

/** The amount moved by the Token-2022 transfer that called this hook: the nearest step above it, one level up. */
function callingTransfer(trace: TraceView, hook: TraceStep): TraceStep | null {
  for (let i = hook.number - 2; i >= 0; i -= 1) {
    const step = trace.steps[i];
    if (step.depth === hook.depth - 1) return step;
    if (step.depth < hook.depth - 1) return null;
  }
  return null;
}

const detail = (steps: TraceStep, name: string): string | undefined => steps.details.find((entry) => entry.name === name)?.value;

function failedCode(step: TraceStep): number | null {
  const match = /custom program error: 0x([0-9a-f]+)/i.exec(step.logs.join('\n'));
  return match ? Number.parseInt(match[1], 16) : null;
}

function fairLaunch(policy: Extract<HookPolicy, { kind: 'fair-launch' }>, trace: TraceView, step: TraceStep): Enforcement {
  const { config, decimals } = policy;
  const tokens = (raw: bigint) => formatAmount(raw, decimals);
  const [source, , destination] = step.accounts;
  const transfer = callingTransfer(trace, step);
  const amount = transfer ? BigInt(detail(transfer, 'amount') ?? '0') : null;
  const isBuy = config.venues.some((venue) => venue.toBase58() === source);
  const headline = isBuy
    ? 'Fair Launch checks: max buy · wallet cap · buys per slot · priority fee · launch window'
    : 'Fair Launch: not a buy, so none of its rules apply';
  if (!isBuy) {
    return { headline, lines: [{ name: 'Buy rules', detail: 'This transfer is not out of a launch pool, so it is a sell or a plain transfer, which Fair Launch never restricts.', status: 'skipped' }] };
  }

  // A refusal names the rule that broke; every other rule had passed or not yet run.
  const refused = step.failed ? decodeFairLaunchError(failedCode(step) ?? -1) : null;
  const result = (broken: string[]): 'passed' | 'failed' => (refused && broken.includes(refused.name) ? 'failed' : 'passed');
  const lines: RuleLine[] = [];
  if (config.maxBuy > 0n) {
    lines.push({
      name: 'Max buy per transaction',
      detail: `${amount === null ? 'this buy' : tokens(amount)} of at most ${tokens(config.maxBuy)}`,
      status: result(['PerBuyCapExceeded']),
    });
  }
  if (config.maxWallet > 0n) {
    const after = destination ? trace.postBalances[destination] : undefined;
    lines.push({
      name: 'Wallet cap after the buy',
      detail: `${after === undefined ? 'the buyer' : tokens(BigInt(after))} of at most ${tokens(config.maxWallet)} held`,
      status: result(['MaxWalletExceeded']),
    });
  }
  if (config.maxBuysPerSlot > 0) {
    lines.push({ name: 'Buys per slot', detail: `at most ${config.maxBuysPerSlot} in one slot, across every pool of this token (the hook counts them)`, status: result(['TooManyBuysInSlot']) });
  }
  if (config.maxPriorityMicroLamports > 0n) {
    lines.push({
      name: 'Priority fee',
      detail: `${trace.computeUnitPrice ?? 0} µ-lamports declared, at most ${config.maxPriorityMicroLamports} allowed`,
      status: result(['PriorityFeeTooHigh']),
    });
  }
  if (trace.blockTime !== null) {
    const open = BigInt(trace.blockTime) >= config.windowStart && BigInt(trace.blockTime) <= config.windowEnd;
    lines.push({
      name: 'Launch window',
      detail: open ? 'inside the window, so the rules above applied' : 'outside the window, so the rules above no longer apply',
      status: open ? 'passed' : 'skipped',
    });
  }
  if (refused) lines.push({ name: 'Refused', detail: refused.message, status: 'failed' });
  return { headline, lines };
}

function creatorCommitment(policy: Extract<HookPolicy, { kind: 'creator-commitment' }>, trace: TraceView, step: TraceStep): Enforcement {
  const { config, decimals } = policy;
  const [source] = step.accounts;
  const headline = 'Creator Commitment checks: the vesting floor on the creator account';
  if (source !== config.creatorAccount.toBase58()) {
    return { headline, lines: [{ name: 'Vesting floor', detail: 'This transfer is not out of the creator account, and the floor binds only that account.', status: 'skipped' }] };
  }
  const locked = trace.blockTime === null ? config.lockedTotal : vestingView(config, BigInt(trace.blockTime)).locked;
  const after = trace.postBalances[source];
  return {
    headline,
    lines: [
      {
        name: 'Vesting floor',
        detail: `${after === undefined ? 'the creator account' : formatAmount(BigInt(after), decimals)} left, at least ${formatAmount(locked, decimals)} still locked`,
        status: step.failed ? 'failed' : 'passed',
      },
    ],
  };
}

/**
 * What this hook enforced on the transfer it ran for, named, with the numbers of this transaction against its
 * limits. The hook program only says "Execute"; its rules come from its config account, which the page has read.
 * A rule the hook broke is marked from the error the hook returned.
 */
export function enforcedRules(policy: HookPolicy | undefined, trace: TraceView, step: TraceStep): Enforcement | null {
  if (!policy || !step.isHook) return null;
  switch (policy.kind) {
    case 'fair-launch':
      return fairLaunch(policy, trace, step);
    case 'creator-commitment':
      return creatorCommitment(policy, trace, step);
    case 'holder-rewards':
      return {
        headline: 'Holder Rewards: keeps the balance × time records exact',
        lines: [
          {
            name: 'Reward accounting',
            detail: 'Updates the reward records of the accounts this transfer moves between, so each holder’s share of the stream stays exact. It never refuses a transfer.',
            status: 'info',
          },
        ],
      };
  }
}
