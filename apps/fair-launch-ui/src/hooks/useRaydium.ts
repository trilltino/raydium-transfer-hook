import type { Raydium } from '@raydium-io/raydium-sdk-v2';
import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { useEffect, useMemo, useState } from 'react';
import { connectionFor, loadRaydium } from '../lib/chain.ts';

/** One RPC connection per environment, and Raydium SDK V2 initialised on it. */
export function useRaydium(environment: HookEnvironment) {
  const connection = useMemo(() => connectionFor(environment), [environment]);
  const [raydium, setRaydium] = useState<Raydium | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setRaydium(null);
    setError(null);
    loadRaydium(connection, environment)
      .then((instance) => live && setRaydium(instance))
      .catch((cause: unknown) => live && setError(cause instanceof Error ? cause.message : String(cause)));
    return () => {
      live = false;
    };
  }, [connection, environment]);

  return { connection, raydium, error };
}
