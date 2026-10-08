import { formatAmount } from '../lib/amounts.ts';
import type { SwapQuote } from '../lib/quote.ts';

export interface SwapSummaryProps {
  quote: SwapQuote;
  inDecimals: number;
  outDecimals: number;
  outLabel: string;
  inLabel: string;
  slippageBps: number;
  hookLabel: string;
  policyLabel: string | null;
  updatedAt: number;
}

export function SwapSummary(props: SwapSummaryProps) {
  const { quote, inDecimals, outDecimals, outLabel, inLabel, slippageBps, hookLabel, policyLabel, updatedAt } = props;
  const impact = quote.priceImpactBps / 100;
  const age = Math.max(0, Math.round((Date.now() - updatedAt) / 1000));
  return (
    <dl className="summary" aria-label="Swap summary">
      <div>
        <dt>Minimum received</dt>
        <dd data-testid="min-received">
          {formatAmount(quote.minimumOut, outDecimals)} {outLabel}
        </dd>
      </div>
      <div>
        <dt>Price impact</dt>
        <dd className={impact >= 5 ? 'danger' : impact >= 1 ? 'warn' : undefined}>{impact < 0.01 ? '<0.01' : impact.toFixed(2)}%</dd>
      </div>
      <div>
        <dt>Estimated fees</dt>
        <dd>
          {formatAmount(quote.tradeFee, inDecimals)} {inLabel}
        </dd>
      </div>
      <div>
        <dt>Slippage tolerance</dt>
        <dd>{(slippageBps / 100).toFixed(2)}%</dd>
      </div>
      <div>
        <dt>Price freshness</dt>
        <dd>{age < 2 ? 'just now' : `${age}s ago`}</dd>
      </div>
      <div>
        <dt>Hook</dt>
        <dd>{hookLabel}</dd>
      </div>
      {policyLabel && (
        <div>
          <dt>Launch Policy</dt>
          <dd>{policyLabel}</dd>
        </div>
      )}
    </dl>
  );
}
