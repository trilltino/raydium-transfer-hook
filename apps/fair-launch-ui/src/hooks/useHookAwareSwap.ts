import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { useWallet } from '@solana/wallet-adapter-react';
import type { Connection } from '@solana/web3.js';
import { useCallback, useState } from 'react';
import type { PoolContext } from '../lib/chain.ts';
import { type SwapOutcome, type SwapPhase, runHookAwareSwap } from '../lib/run-swap.ts';

export type SwapState = { phase: 'idle' } | { phase: SwapPhase } | { phase: 'done'; outcome: SwapOutcome };

/** The Swap button's state machine: prepare, simulate, sign, confirm; ends `done` with the outcome. */
export function useHookAwareSwap(
  connection: Connection,
  environment: HookEnvironment,
  reload: () => Promise<PoolContext>,
  onFinished: () => void
) {
  const { publicKey, signTransaction } = useWallet();
  const [state, setState] = useState<SwapState>({ phase: 'idle' });

  const run = useCallback(
    async (inputIsA: boolean, amountIn: bigint, slippageBps: number) => {
      if (!publicKey || !signTransaction) return;
      setState({ phase: 'preparing' });
      const outcome = await runHookAwareSwap({
        connection,
        environment,
        payer: publicKey,
        inputIsA,
        amountIn,
        slippageBps,
        reload,
        signTransaction: signTransaction as never,
        onPhase: (phase) => setState({ phase }),
      });
      setState({ phase: 'done', outcome });
      if (outcome.status === 'success') onFinished();
    },
    [connection, environment, onFinished, publicKey, reload, signTransaction]
  );

  return { state, run, reset: () => setState({ phase: 'idle' }) };
}
