import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { useTransactionTrace } from '../hooks/useTransactionTrace.ts';
import { solscanUrl } from '../lib/solscan.ts';
import type { TraceSource } from '../lib/trace-client.ts';
import type { TraceStep, TraceView } from '../lib/trace.ts';

const short = (value: string): string => (value.length > 12 ? `${value.slice(0, 5)}…${value.slice(-5)}` : value);

function Link({ environment, kind, value, label }: { environment: HookEnvironment; kind: 'account' | 'tx' | 'token'; value: string; label?: string }) {
  return (
    <a href={solscanUrl(environment, kind, value)} target="_blank" rel="noreferrer" className="mono">
      {label ?? short(value)}
    </a>
  );
}

function Step({ step, environment }: { step: TraceStep; environment: HookEnvironment }) {
  return (
    <li
      className={`trace-step${step.failed ? ' trace-failed' : ''}${step.isHook ? ' trace-hook' : ''}`}
      style={{ marginLeft: `${Math.max(step.depth - 1, 0) * 14}px` }}
      data-testid="trace-step"
      data-depth={step.depth}
    >
      <div className="trace-line">
        <span className="trace-number">{step.number}</span>
        <span className="trace-program">{step.programName}</span>
        {step.instruction && <span className="trace-instruction">{step.instruction}</span>}
        {step.computeUnits !== null && <span className="trace-cu">{step.computeUnits.toLocaleString('en-US')} CU</span>}
        {step.failed && <span className="trace-badge">failed</span>}
        <Link environment={environment} kind="account" value={step.programId} label="program ↗" />
      </div>
      {step.details.length > 0 && (
        <div className="trace-token muted small">{step.details.map((detail) => `${detail.name}: ${short(detail.value)}`).join(' · ')}</div>
      )}
      {(step.accounts.length > 0 || step.logs.length > 0) && (
        <details className="trace-more">
          <summary>
            {step.accounts.length} accounts · {step.logs.length} log lines
          </summary>
          {step.accounts.length > 0 && (
            <ul className="trace-accounts">
              {step.accounts.map((account, i) => (
                <li key={`${account}-${i}`}>
                  <span className="muted">{i}</span> <Link environment={environment} kind="account" value={account} />
                </li>
              ))}
            </ul>
          )}
          {step.logs.length > 0 && <pre className="mono small">{step.logs.join('\n')}</pre>}
        </details>
      )}
    </li>
  );
}

function Ready({ trace, environment }: { trace: TraceView; environment: HookEnvironment }) {
  const units = (value: number) => value.toLocaleString('en-US');
  return (
    <>
      <p className="trace-summary" data-testid="trace-summary">
        {trace.success ? 'Landed' : 'Failed'} in slot {trace.slot} · read from {trace.source} · {trace.steps.length} steps
        {trace.computeUnits !== null && ` · ${units(trace.computeUnits)}${trace.computeLimit ? ` of ${units(trace.computeLimit)}` : ''} CU`} · fee{' '}
        {units(trace.feeLamports)} lamports
      </p>
      {trace.error && <p className="notice notice-error">{trace.error}</p>}
      {trace.hookRuns.length > 0 && (
        <p className="trace-hooks" data-testid="trace-hooks">
          {trace.hookRuns.map((run) => `${run.programName} ran ${run.count} time${run.count === 1 ? '' : 's'}`).join(' · ')}
        </p>
      )}
      <ol className="trace-steps">
        {trace.steps.map((step) => (
          <Step key={step.id} step={step} environment={environment} />
        ))}
      </ol>
    </>
  );
}

/**
 * The on-chain steps of a landed transaction, read from the cluster (devnet through our Triton One
 * endpoint): every program that ran, nested as the runtime ran them, with its compute, its accounts and its
 * logs, and a Solscan link for the transaction and for each program and account.
 */
export function TransactionTrace({ environment, signature, source }: { environment: HookEnvironment; signature: string; source?: TraceSource }) {
  const state = useTransactionTrace(environment, signature, source);
  return (
    <section className="card trace" data-testid="trace">
      <div className="trace-head">
        <h3>Transaction trace</h3>
        <a href={solscanUrl(environment, 'tx', signature)} target="_blank" rel="noreferrer" data-testid="solscan-tx">
          Open on Solscan ↗
        </a>
      </div>
      {state.status === 'loading' && (
        <p className="muted" role="status" data-testid="trace-loading">
          Reading the transaction{state.attempt > 1 ? ` (try ${state.attempt} of ${state.of}, waiting for the RPC to index it)` : ''}…
        </p>
      )}
      {state.status === 'ready' && <Ready trace={state.trace} environment={environment} />}
      {(state.status === 'unavailable' || state.status === 'not-observed') && (
        <p className="muted" data-testid="trace-note">
          {state.message}
        </p>
      )}
    </section>
  );
}
