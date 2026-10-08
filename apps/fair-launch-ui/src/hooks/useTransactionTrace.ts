import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { useEffect, useState } from 'react';
import { type TraceResult, type TraceSource, traceTransaction } from '../lib/trace-client.ts';

export type TraceState = { status: 'loading'; attempt: number; of: number } | TraceResult;

/** Trace a landed transaction through the local debugger once per signature. */
export function useTransactionTrace(environment: HookEnvironment, signature: string, source?: TraceSource): TraceState {
  const [state, setState] = useState<TraceState>({ status: 'loading', attempt: 0, of: 0 });
  useEffect(() => {
    let cancelled = false;
    setState({ status: 'loading', attempt: 0, of: 0 });
    void traceTransaction({
      environment,
      signature,
      source,
      onAttempt: (attempt, of) => {
        if (!cancelled) setState({ status: 'loading', attempt, of });
      },
    }).then((result) => {
      if (!cancelled) setState(result);
    });
    return () => {
      cancelled = true;
    };
  }, [environment, signature, source]);
  return state;
}
