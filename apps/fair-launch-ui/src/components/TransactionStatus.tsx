import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { explorerUrl } from '../config.ts';
import type { HookPolicy } from '../lib/hook-rules.ts';
import { TransactionTrace } from './TransactionTrace.tsx';
import type { SwapState } from '../hooks/useHookAwareSwap.ts';

const PHASE_TEXT: Record<string, string> = {
  preparing: 'Preparing: reloading the pool and resolving hook accounts…',
  simulating: 'Simulating before you sign…',
  'awaiting-signature': 'Simulation passed. Confirm in your wallet…',
  confirming: 'Submitted. Waiting for confirmation…',
};

export function TransactionStatus({ state, environment, policy }: { state: SwapState; environment: HookEnvironment; policy?: HookPolicy }) {
  if (state.phase === 'idle') return null;
  if (state.phase !== 'done') {
    return (
      <div className="status status-pending" role="status" aria-live="polite" data-testid="tx-status">
        {PHASE_TEXT[state.phase]}
      </div>
    );
  }
  const { outcome } = state;
  if (outcome.status === 'success') {
    return (
      <>
        <div className="status status-ok" role="status" aria-live="polite" data-testid="tx-status">
          <strong>Swap confirmed</strong>
          <a href={explorerUrl(environment, 'tx', outcome.signature)} target="_blank" rel="noreferrer" className="mono wrap">
            {outcome.signature}
          </a>
        </div>
        <TransactionTrace environment={environment} signature={outcome.signature} policy={policy} />
      </>
    );
  }
  const { failure } = outcome;
  return (
    <div className="status status-blocked" role="alert" data-testid="tx-status">
      <strong>{failure.title}</strong>
      <div className="reason">
        <span className="label">Reason</span>
        <span data-testid="failure-reason">{failure.reason}</span>
      </div>
      {failure.notSubmitted && <span className="muted">No transaction was submitted.</span>}
    </div>
  );
}
