import {
  type HookFailure,
  HookClientError,
  decodeFairLaunchError,
  type FairLaunchErrorInfo,
} from '@raydium-transfer-hook/client';

/** What the UI shows when a swap cannot go ahead. Raw detail is for the Developer details panel only. */
export interface FailureView {
  /** `hook` blocks come from a Transfer Hook program; the rest are other problems. */
  source: 'fair-launch' | 'hook' | 'raydium' | 'client' | 'network';
  title: string;
  reason: string;
  /** Always true here: failures are found by simulation, before any signature is requested. */
  notSubmitted: boolean;
  codeHex?: string;
  /** Machine-readable rule name, e.g. `MaxWalletExceeded`. */
  rule?: string;
  raw: string;
}

export interface PresentContext {
  fairLaunchProgramId: string;
}

/**
 * Turn a decoded simulation failure into words. Only a failure raised by the fair-launch program gets the
 * Fair Launch mapping; a code from any other hook is reported as unknown, never guessed at.
 */
export function presentSimulationFailure(failure: HookFailure, context: PresentContext, logs: readonly string[] = []): FailureView {
  const raw = [JSON.stringify(failure), ...logs].join('\n');
  if (failure.kind === 'hook') {
    if (failure.programId === context.fairLaunchProgramId) {
      const info: FairLaunchErrorInfo | null = decodeFairLaunchError(failure.code);
      if (info) {
        return {
          source: 'fair-launch',
          title: 'Swap blocked by Fair Launch',
          reason: info.message,
          notSubmitted: true,
          codeHex: failure.codeHex,
          rule: info.name,
          raw,
        };
      }
    }
    return {
      source: 'hook',
      title: 'Transfer Hook rejected this transaction.',
      reason: `Error code: ${failure.codeHex}. No known Fair Launch mapping exists for this code.`,
      notSubmitted: true,
      codeHex: failure.codeHex,
      raw,
    };
  }
  if (failure.kind === 'program') {
    return {
      source: 'raydium',
      title: 'The swap was rejected',
      reason: `A program returned error ${failure.codeHex}. Try a smaller amount or a higher slippage tolerance.`,
      notSubmitted: true,
      codeHex: failure.codeHex,
      raw,
    };
  }
  return {
    source: 'network',
    title: 'The swap could not be simulated',
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
