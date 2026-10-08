import { type CreatorCommitmentConfig, vestingView } from '@raydium-transfer-hook/client';
import { formatAmount } from '../lib/amounts.ts';
import { PolicyMeter } from './PolicyMeter.tsx';

/** What a sale out of the creator's account would do to the floor, when this swap is such a sale. */
export interface FloorCheck {
  locked: bigint;
  balanceAfter: bigint;
  violated: boolean;
}

export interface CreatorCommitmentPolicyProps {
  config: CreatorCommitmentConfig;
  now: bigint;
  decimals: number;
  hookProgramId: string;
  tokenLabel: string;
  /** Set when the swap being typed sells from the creator account. */
  floorCheck: FloorCheck | null;
  /** Whether the connected wallet's account is the creator account. */
  walletIsCreator: boolean;
}

function when(seconds: bigint): string {
  return new Date(Number(seconds) * 1000).toLocaleString();
}

function duration(seconds: bigint): string {
  const total = Number(seconds < 0n ? 0n : seconds);
  const d = Math.floor(total / 86_400);
  const h = Math.floor((total % 86_400) / 3600);
  const m = Math.floor((total % 3600) / 60);
  return d > 0 ? `${d}d ${h}h` : h > 0 ? `${h}h ${m}m` : m > 0 ? `${m}m ${total % 60}s` : `${total}s`;
}

/** The creator's vesting schedule: how much is locked now, and when the rest unlocks. */
export function CreatorCommitmentPolicy(props: CreatorCommitmentPolicyProps) {
  const { config, now, decimals, hookProgramId, tokenLabel, floorCheck, walletIsCreator } = props;
  const view = vestingView(config, now);
  const tokens = (value: bigint) => formatAmount(value, decimals);
  const headline =
    view.phase === 'before-cliff'
      ? `Locked until the cliff · ${duration(config.cliff - now)} to go`
      : view.phase === 'vesting'
        ? `Vesting · ${duration(config.end - now)} until fully unlocked`
        : 'Fully vested';
  return (
    <section className="card policy" aria-labelledby="commitment-title">
      <h2 id="commitment-title">Creator commitment</h2>
      <p className={`policy-phase phase-${view.phase === 'complete' ? 'ended' : view.phase === 'vesting' ? 'active' : 'not-started'}`} data-testid="policy-phase">
        {headline}
      </p>
      <div className="meters">
        <PolicyMeter
          label="Vesting progress"
          usedText={`${Math.round(view.elapsed * 100)}%`}
          limitText="100%"
          fraction={view.elapsed}
          violated={false}
        />
      </div>
      <dl className="kv">
        <div>
          <dt>Locked in total</dt>
          <dd data-testid="locked-total">{tokens(config.lockedTotal)}</dd>
        </div>
        <div>
          <dt>Locked now</dt>
          <dd data-testid="locked-now">{tokens(view.locked)}</dd>
        </div>
        <div>
          <dt>Unlocked</dt>
          <dd>{tokens(view.unlocked)}</dd>
        </div>
        <div>
          <dt>Cliff</dt>
          <dd>{when(config.cliff)}</dd>
        </div>
        <div>
          <dt>Fully unlocked</dt>
          <dd>{when(config.end)}</dd>
        </div>
        <div>
          <dt>Creator account</dt>
          <dd className="mono wrap" data-testid="creator-account">
            {config.creatorAccount.toBase58()}
          </dd>
        </div>
        <div>
          <dt>Token</dt>
          <dd>{tokenLabel}</dd>
        </div>
        <div>
          <dt>Hook program</dt>
          <dd className="mono wrap">{hookProgramId}</dd>
        </div>
      </dl>
      <p className="muted small">
        The creator account cannot fall below what is still locked, whoever holds it. Anyone else trades this token freely.
      </p>
      {walletIsCreator && <p className="buy-banner">YOUR WALLET IS THE CREATOR ACCOUNT</p>}
      {floorCheck && (
        <p className={floorCheck.violated ? 'warn-box' : 'muted small'} role="status" data-testid="floor-check" data-violated={floorCheck.violated}>
          {floorCheck.violated
            ? `This sale would leave ${tokens(floorCheck.balanceAfter)}, below the ${tokens(floorCheck.locked)} still locked. The on-chain hook will reject it.`
            : `After this sale the creator account holds ${tokens(floorCheck.balanceAfter)}, at or above the ${tokens(floorCheck.locked)} still locked.`}
        </p>
      )}
    </section>
  );
}
