import {
  type HookEnvironment,
  type HookFailure,
  HookClientError,
  decodeCreatorCommitmentError,
  decodeFairLaunchError,
  decodeRewardsError,
} from '@raydium-transfer-hook/client';

/** What the UI shows when something cannot go ahead. Raw detail is for the Developer details panel only. */
export interface FailureView {
  /** `hook` blocks come from a Transfer Hook program; the rest are other problems. */
  source: 'fair-launch' | 'creator-commitment' | 'holder-rewards' | 'hook' | 'raydium' | 'client' | 'network';
  title: string;
  reason: string;
  /** Always true here: failures are found by simulation, before any signature is requested. */
  notSubmitted: boolean;
  codeHex?: string;
  /** Machine-readable rule name, e.g. `MaxWalletExceeded`. */
  rule?: string;
  raw: string;
}

/** The program ids of the example hooks this page can explain; any other hook's codes are reported as unknown. */
export interface PresentContext {
  fairLaunchProgramId: string;
  creatorCommitmentProgramId?: string;
  holderRewardsProgramId?: string;
  /** What was being attempted, for the fallback wording: `swap` (the default) or `action`. */
  subject?: 'swap' | 'action';
}

/** The example hooks of `environment`, so a refusal by one of them is explained in words. */
export function presentContext(environment: HookEnvironment, subject: 'swap' | 'action' = 'swap'): PresentContext {
  return {
    fairLaunchProgramId: environment.fairLaunchProgramId,
    creatorCommitmentProgramId: environment.creatorCommitmentProgramId,
    holderRewardsProgramId: environment.holderRewardsProgramId,
    subject,
  };
}

/**
 * Turn a decoded simulation failure into words. Only a failure raised by one of the example programs
 * gets that program's mapping; a code from any other hook is reported as unknown, never guessed at.
 */
export function presentSimulationFailure(failure: HookFailure, context: PresentContext, logs: readonly string[] = []): FailureView {
  const raw = [JSON.stringify(failure), ...logs].join('\n');
  const subject = context.subject ?? 'swap';
  if (failure.kind === 'hook') {
    const base = { notSubmitted: true, codeHex: failure.codeHex, raw };
    if (failure.programId === context.fairLaunchProgramId) {
      const info = decodeFairLaunchError(failure.code);
      if (info) return { ...base, source: 'fair-launch', title: 'Swap blocked by Fair Launch', reason: info.message, rule: info.name };
    }
    if (context.creatorCommitmentProgramId && failure.programId === context.creatorCommitmentProgramId) {
      const info = decodeCreatorCommitmentError(failure.code);
      if (info) {
        return { ...base, source: 'creator-commitment', title: 'Blocked by the creator’s vesting schedule', reason: info.message, rule: info.name };
      }
    }
    if (context.holderRewardsProgramId && failure.programId === context.holderRewardsProgramId) {
      const info = decodeRewardsError(failure.code);
      if (info) {
        return { ...base, source: 'holder-rewards', title: subject === 'swap' ? 'Swap blocked by the rewards hook' : 'Rewards action blocked', reason: info.message, rule: info.name };
      }
    }
    return {
      ...base,
      source: 'hook',
      title: 'Transfer Hook rejected this transaction.',
      reason: `Error code: ${failure.codeHex}. No known mapping exists for this code.`,
    };
  }
  if (failure.kind === 'program') {
    return {
      source: 'raydium',
      title: subject === 'swap' ? 'The swap was rejected' : 'The transaction was rejected',
      reason:
        subject === 'swap'
          ? `A program returned error ${failure.codeHex}. Try a smaller amount or a higher slippage tolerance.`
          : `A program returned error ${failure.codeHex}.`,
      notSubmitted: true,
      codeHex: failure.codeHex,
      raw,
    };
  }
  return {
    source: 'network',
    title: subject === 'swap' ? 'The swap could not be simulated' : 'The transaction could not be simulated',
    reason: failure.message,
    notSubmitted: true,
    raw,
  };
}

/** A refusal the client made before building anything (wrong hook program, writable extra, ...). */
export function presentClientError(error: unknown): FailureView {
  if (error instanceof HookClientError) {
    return {
      source: 'client',
      title: 'The hook accounts were refused',
      reason: error.message,
      notSubmitted: true,
      rule: error.kind,
      raw: `${error.kind}: ${error.message}`,
    };
  }
  const message = error instanceof Error ? error.message : String(error);
  return { source: 'network', title: 'Something went wrong', reason: message, notSubmitted: true, raw: message };
}
