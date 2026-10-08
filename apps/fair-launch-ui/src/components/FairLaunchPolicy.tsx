import type { FairLaunchConfig, FairLaunchCounter } from '@raydium-transfer-hook/client';
import type { LaunchView } from '../hooks/useFairLaunch.ts';
import { formatAmount } from '../lib/amounts.ts';
import { PolicyMeter } from './PolicyMeter.tsx';

export interface FairLaunchPolicyProps {
  config: FairLaunchConfig;
  counter: FairLaunchCounter | null;
  view: LaunchView | null;
  decimals: number;
  hookProgramId: string;
  tokenLabel: string;
  /** Fill the swap box with a buy that is inside the limits, or one over the per-buy limit, to see the hook's answer. */
  onTry?: (kind: 'allowed' | 'over-limit') => void;
}

function duration(seconds: bigint): string {
  const total = Number(seconds < 0n ? 0n : seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0 ? `${h}h ${m}m` : m > 0 ? `${m}m ${s}s` : `${s}s`;
}

/** Number of switched-on protections: a limit of 0 means that check is off. */
export function protectionCount(config: FairLaunchConfig): number {
  return [config.maxBuy > 0n, config.maxWallet > 0n, config.maxBuysPerSlot > 0, config.maxPriorityMicroLamports > 0n].filter(Boolean)
    .length;
}

export function FairLaunchPolicy({ config, counter, view, decimals, hookProgramId, tokenLabel, onTry }: FairLaunchPolicyProps) {
  const phase = view?.phase ?? 'active';
  const rowFor = (rule: string) => view?.rows.find((row) => row.rule === rule);
  const meter = (rule: 'max-buy' | 'max-wallet' | 'buys-per-slot' | 'priority-fee', label: string, limit: bigint, unit: (v: bigint) => string) => {
    const row = rowFor(rule);
    return (
      <PolicyMeter
        key={rule}
        label={label}
        usedText={row ? unit(row.used) : '—'}
        limitText={unit(limit)}
        fraction={row && limit > 0n ? Number(row.used) / Number(limit) : 0}
        violated={row?.violated ?? false}
      />
    );
  };
  const tokens = (v: bigint) => formatAmount(v, decimals);
  const plain = (v: bigint) => v.toString();
  // What each switched-on rule means, in words: the page shows numbers, this says what they are for.
  const rules: { key: string; text: string }[] = [];
  if (config.maxBuy > 0n) rules.push({ key: 'max-buy', text: `No single buy may take more than ${tokens(config.maxBuy)} ${tokenLabel}, so one sniper cannot sweep the launch in a trade.` });
  if (config.maxWallet > 0n) rules.push({ key: 'max-wallet', text: `No wallet may hold more than ${tokens(config.maxWallet)} ${tokenLabel} after a buy.` });
  if (config.maxBuysPerSlot > 0) {
    rules.push({ key: 'per-slot', text: `At most ${config.maxBuysPerSlot} buys per slot, across every pool of this token: a bundle with more is refused as a whole.` });
  }
  if (config.maxPriorityMicroLamports > 0n) {
    rules.push({ key: 'priority', text: `A buy that pays a priority fee above ${config.maxPriorityMicroLamports.toString()} µ-lamports per compute unit is refused, so fee wars do not decide who gets in.` });
  }

  return (
    <section className="card policy" aria-labelledby="policy-title">
      <h2 id="policy-title">Fair Launch policy</h2>
      <p className={`policy-phase phase-${phase}`} data-testid="policy-phase">
        {phase === 'active' && `Active${view?.secondsLeft != null ? ` · ${duration(view.secondsLeft)} remaining` : ''}`}
        {phase === 'not-started' && `Not started${view?.secondsLeft != null ? ` · starts in ${duration(view.secondsLeft)}` : ''}`}
        {phase === 'ended' && 'Fair Launch window ended'}
      </p>
      {phase === 'ended' ? (
        <p className="muted">Transfers are no longer restricted by this policy.</p>
      ) : (
        <>
          {view?.isBuy && <p className="buy-banner">BUY PROTECTIONS ACTIVE</p>}
          {view && !view.isBuy && phase === 'active' && (
            <p className="muted">Fair Launch buy restrictions do not apply to this sell.</p>
          )}
          <div className="meters">
            {config.maxBuy > 0n && meter('max-buy', 'Buy amount', config.maxBuy, tokens)}
            {config.maxWallet > 0n && meter('max-wallet', 'Wallet after', config.maxWallet, tokens)}
            {config.maxBuysPerSlot > 0 && meter('buys-per-slot', 'Buys this slot', BigInt(config.maxBuysPerSlot), plain)}
            {config.maxPriorityMicroLamports > 0n ? (
              meter('priority-fee', 'Priority fee (µ-lamports)', config.maxPriorityMicroLamports, plain)
            ) : (
              <p className="muted">Priority fee rule disabled</p>
            )}
          </div>
          {rules.length > 0 && (
            <div className="policy-rules" data-testid="policy-rules">
              <h3>What this launch enforces</h3>
              <ul>
                {rules.map((rule) => (
                  <li key={rule.key}>{rule.text}</li>
                ))}
                <li>Selling is never restricted.</li>
              </ul>
              {onTry && config.maxBuy > 0n && (
                <div className="policy-try">
                  <span className="muted small">Try it, then press Swap:</span>
                  <button type="button" className="btn btn-ghost" onClick={() => onTry('allowed')}>
                    A buy inside the limits
                  </button>
                  <button type="button" className="btn btn-ghost" onClick={() => onTry('over-limit')}>
                    A buy over the limit
                  </button>
                </div>
              )}
            </div>
          )}
          {counter && (
            <p className="muted small">
              Last buy slot {counter.slot.toString()} · {counter.buys} buy{counter.buys === 1 ? '' : 's'}
            </p>
          )}
        </>
      )}
      <dl className="kv">
        <div>
          <dt>Token</dt>
          <dd>{tokenLabel}</dd>
        </div>
        <div>
          <dt>Hook program</dt>
          <dd className="mono wrap">{hookProgramId}</dd>
        </div>
      </dl>
    </section>
  );
}
