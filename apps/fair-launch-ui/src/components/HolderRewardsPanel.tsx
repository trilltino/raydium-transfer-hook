import {
  type HookEnvironment,
  buildClaimInstruction,
  buildRegisterInstruction,
  claimableAt,
  rewardsView,
} from '@raydium-transfer-hook/client';
import { createAssociatedTokenAccountIdempotentInstruction, getAssociatedTokenAddressSync } from '@solana/spl-token';
import { useWallet } from '@solana/wallet-adapter-react';
import type { Connection, PublicKey } from '@solana/web3.js';
import { useState } from 'react';
import { useNow } from '../hooks/useNow.ts';
import { useRewardsAccount } from '../hooks/useRewardsAccount.ts';
import { explorerUrl } from '../config.ts';
import { TransactionTrace } from './TransactionTrace.tsx';
import { formatAmount } from '../lib/amounts.ts';
import type { PoolContext, RewardsState } from '../lib/chain.ts';
import { hookProgramId } from '../lib/hooks.ts';
import { presentContext } from '../lib/present.ts';
import { type TransactionOutcome, runTransaction } from '../lib/run-transaction.ts';

export interface HolderRewardsPanelProps {
  environment: HookEnvironment;
  connection: Connection;
  context: PoolContext;
  rewards: RewardsState;
  tokenLabel: string;
  /** Called after a register or a claim lands, so balances and the stream are reloaded. */
  onChanged: () => void;
}

type Action = 'register' | 'claim';

/**
 * The holder-rewards panel: the stream the token pays, this wallet's account in it, and the two actions
 * that are ordinary instructions of the hook program (Register and Claim), not part of the swap. Both
 * are simulated before the wallet is asked to sign, like a swap.
 */
export function HolderRewardsPanel({ environment, connection, context, rewards, tokenLabel, onChanged }: HolderRewardsPanelProps) {
  const { publicKey, signTransaction } = useWallet();
  const now = useNow();
  const pool = context.pool;
  const hookedToken = rewards.hookedSide === 'A' ? pool.tokenA : pool.tokenB;
  const program = hookProgramId(environment, 'holder-rewards');
  const { account, error, refresh } = useRewardsAccount(connection, rewards, hookedToken.mint, hookedToken.tokenProgram, program, publicKey);
  const [busy, setBusy] = useState<Action | null>(null);
  const [outcome, setOutcome] = useState<{ action: Action; result: TransactionOutcome } | null>(null);

  const rewardDecimals = rewards.rewardMint.decimals;
  const registered = account?.record != null;
  const claimable = account?.record ? claimableAt(rewards.global, account.record, account.balance, now) : 0n;
  const view = rewardsView(rewards.global, account?.balance ?? 0n, registered, now);
  const reward = (value: bigint) => formatAmount(value, rewardDecimals);

  async function run(action: Action) {
    if (!publicKey || !signTransaction || !account) return;
    setBusy(action);
    setOutcome(null);
    const instructions =
      action === 'register'
        ? [buildRegisterInstruction(program, publicKey, hookedToken.mint, account.tokenAccount)]
        : (() => {
            const rewardAccount = getAssociatedTokenAddressSync(rewards.rewardMint.mint, publicKey, false, rewards.rewardMint.tokenProgram);
            return [
              createAssociatedTokenAccountIdempotentInstruction(publicKey, rewardAccount, publicKey, rewards.rewardMint.mint, rewards.rewardMint.tokenProgram),
              buildClaimInstruction(
                program,
                publicKey,
                hookedToken.mint,
                account.tokenAccount,
                rewardAccount,
                rewards.rewardMint.mint,
                rewards.rewardMint.tokenProgram
              ),
            ];
          })();
    const result = await runTransaction({
      connection,
      payer: publicKey,
      instructions,
      hookPrograms: [program],
      signTransaction: signTransaction as never,
      present: presentContext(environment, 'action'),
    });
    setOutcome({ action, result });
    setBusy(null);
    if (result.status === 'success') {
      await refresh();
      onChanged();
    }
  }

  const poolHolds = (key: PublicKey) => key.equals(rewards.global.poolVault);

  return (
    <section className="card policy" aria-labelledby="rewards-title">
      <h2 id="rewards-title">Holder rewards</h2>
      <p className={`policy-phase phase-${view.funded && view.secondsLeft > 0n ? 'active' : 'ended'}`} data-testid="policy-phase">
        {!view.funded
          ? 'Not funded yet'
          : view.secondsLeft > 0n
            ? `Paying ${reward(view.ratePerSecond)} ${'per second'} · ${Number(view.secondsLeft)}s left`
            : 'The funded period has ended'}
      </p>
      <dl className="kv">
        <div>
          <dt>Mode</dt>
          <dd>{rewards.global.oneTime ? 'One-time allocation' : 'Ongoing programme'}</dd>
        </div>
        <div>
          <dt>Registered balance in the stream</dt>
          <dd data-testid="eligible-supply">{formatAmount(rewards.global.stream.eligibleSupply, hookedToken.decimals)}</dd>
        </div>
        <div>
          <dt>Reward token</dt>
          <dd className="mono wrap">{rewards.rewardMint.mint.toBase58()}</dd>
        </div>
        <div>
          <dt>Token</dt>
          <dd>{tokenLabel}</dd>
        </div>
      </dl>

      <h3 className="subhead">Your account</h3>
      {!publicKey && <p className="muted">Connect a wallet to see your rewards.</p>}
      {publicKey && account && (
        <>
          <dl className="kv">
            <div>
              <dt>Registered</dt>
              <dd data-testid="registered">{registered ? 'Yes' : 'No'}</dd>
            </div>
            <div>
              <dt>Your balance</dt>
              <dd>{formatAmount(account.balance, hookedToken.decimals)}</dd>
            </div>
            <div>
              <dt>Your share of the stream</dt>
              <dd>{registered ? `${(Number(view.sharePpm) / 10_000).toFixed(2)}%` : '—'}</dd>
            </div>
            <div>
              <dt>Settled, not yet claimed</dt>
              <dd>{reward(account.record?.earned ?? 0n)}</dd>
            </div>
            <div>
              <dt>Claimable now</dt>
              <dd data-testid="claimable">{reward(claimable)}</dd>
            </div>
            <div>
              <dt>Your reward balance</dt>
              <dd data-testid="reward-balance">{reward(account.rewardBalance)}</dd>
            </div>
          </dl>
          <div className="actions">
            <button
              type="button"
              className="btn btn-ghost"
              disabled={busy !== null || registered || account.balance === 0n || poolHolds(account.tokenAccount)}
              onClick={() => void run('register')}
            >
              {busy === 'register' ? 'Registering…' : 'Register'}
            </button>
            <button
              type="button"
              className="btn btn-primary"
              disabled={busy !== null || !registered || claimable === 0n}
              onClick={() => void run('claim')}
            >
              {busy === 'claim' ? 'Claiming…' : 'Claim'}
            </button>
          </div>
          {!registered && account.balance === 0n && <p className="muted small">Hold some of this token first: only an account that holds it can register.</p>}
          {!registered && account.balance > 0n && (
            <p className="muted small">Only registered accounts earn. Registering costs a small rent deposit and nothing else.</p>
          )}
        </>
      )}
      {error && <p className="notice notice-error">{error}</p>}
      {outcome && <ActionResult environment={environment} outcome={outcome.result} action={outcome.action} />}
    </section>
  );
}

function ActionResult({ environment, outcome, action }: { environment: HookEnvironment; outcome: TransactionOutcome; action: Action }) {
  if (outcome.status === 'success') {
    return (
      <>
        <div className="status status-ok" role="status" data-testid="action-status">
          <strong>{action === 'register' ? 'Registered' : 'Rewards claimed'}</strong>
          <a href={explorerUrl(environment, 'tx', outcome.signature)} target="_blank" rel="noreferrer" className="mono wrap">
            {outcome.signature}
          </a>
        </div>
        <TransactionTrace environment={environment} signature={outcome.signature} />
      </>
    );
  }
  return (
    <div className="status status-blocked" role="alert" data-testid="action-status">
      <strong>{outcome.failure.title}</strong>
      <div className="reason">
        <span className="label">Reason</span>
        <span data-testid="failure-reason">{outcome.failure.reason}</span>
      </div>
      <span className="muted">No transaction was submitted.</span>
    </div>
  );
}
