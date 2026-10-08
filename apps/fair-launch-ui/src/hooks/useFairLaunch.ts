import { type PolicyRow, isLaunchBuy, launchPhase, previewBuy } from '@raydium-transfer-hook/client';
import { useEffect, useState } from 'react';
import type { PoolContext } from '../lib/chain.ts';
import type { SwapQuote } from '../lib/quote.ts';

export interface LaunchView {
  phase: ReturnType<typeof launchPhase>;
  /** True if this swap is a buy of the launch token (it leaves one of the launch venues). */
  isBuy: boolean;
  rows: PolicyRow[];
  violated: PolicyRow[];
  /** Seconds until the window ends (active) or starts (not started); null once it ended. */
  secondsLeft: bigint | null;
}

const nowSeconds = (): bigint => BigInt(Math.floor(Date.now() / 1000));

/** The launch's state for the swap being typed: phase, countdown and each limit against this buy. */
export function useFairLaunch(
  context: PoolContext | null,
  inputIsA: boolean,
  quote: SwapQuote | null,
  hookedBalance: bigint,
  priorityMicroLamports?: bigint
): LaunchView | null {
  const [now, setNow] = useState(nowSeconds);
  useEffect(() => {
    const timer = setInterval(() => setNow(nowSeconds()), 1000);
    return () => clearInterval(timer);
  }, []);

  const launch = context?.launch;
  if (!context || !launch) return null;
  const pool = context.pool;
  const outputVault = inputIsA ? pool.tokenB.vault : pool.tokenA.vault;
  const isBuy = isLaunchBuy(launch.config, outputVault);
  const phase = launchPhase(launch.config, now);
  const rows =
    isBuy && quote
      ? previewBuy(
          launch.config,
          launch.counter,
          { amount: quote.amountOut, walletBalance: hookedBalance, currentSlot: context.slot, priorityMicroLamports },
          now
        )
      : [];
  return {
    phase,
    isBuy,
    rows,
    violated: rows.filter((row) => row.violated),
    secondsLeft:
      phase === 'active' ? launch.config.windowEnd - now : phase === 'not-started' ? launch.config.windowStart - now : null,
  };
}
