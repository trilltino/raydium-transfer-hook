import { type HookEnvironment, previewCreatorTransfer } from '@raydium-transfer-hook/client';
import { getAssociatedTokenAddressSync } from '@solana/spl-token';
import { useWallet } from '@solana/wallet-adapter-react';
import type { Connection, PublicKey } from '@solana/web3.js';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useFairLaunch } from '../hooks/useFairLaunch.ts';
import { useHookAwareSwap } from '../hooks/useHookAwareSwap.ts';
import { useNow } from '../hooks/useNow.ts';
import { useSwapQuote } from '../hooks/useSwapQuote.ts';
import type { Balances } from '../hooks/useWalletBalances.ts';
import { formatAmount, parseAmount, shortKey, toInputText } from '../lib/amounts.ts';
import type { PoolContext } from '../lib/chain.ts';
import { type KnownHook, hookSummary, hookedToken } from '../lib/hooks.ts';
import { CreatorCommitmentPolicy, type FloorCheck } from './CreatorCommitmentPolicy.tsx';
import { DeveloperDetails } from './DeveloperDetails.tsx';
import { HolderRewardsPanel } from './HolderRewardsPanel.tsx';
import { FairLaunchPolicy, protectionCount } from './FairLaunchPolicy.tsx';
import { SwapSummary } from './SwapSummary.tsx';
import { TokenAmountInput } from './TokenAmountInput.tsx';
import { TransactionStatus } from './TransactionStatus.tsx';
import { UnknownTokenGate, readConfirmed, rememberConfirmed } from './UnknownTokenGate.tsx';

export const SLIPPAGE_OPTIONS_BPS = [10, 50, 100] as const;

export interface ButtonState {
  label: string;
  disabled: boolean;
}

/** What the primary button says and whether it can be pressed. Rule violations do NOT disable it: the simulation shows the hook's own refusal. */
export function swapButtonState(input: {
  connected: boolean;
  amount: bigint | null;
  balance: bigint | null;
  hasQuote: boolean;
  gated: boolean;
  busy: boolean;
}): ButtonState {
  if (!input.connected) return { label: 'Connect wallet', disabled: true };
  if (input.busy) return { label: 'Swapping…', disabled: true };
  if (input.amount === null || input.amount <= 0n || !input.hasQuote) return { label: 'Enter an amount', disabled: true };
  if (input.balance !== null && input.amount > input.balance) return { label: 'Insufficient balance', disabled: true };
  if (input.gated) return { label: 'Confirm the token first', disabled: true };
  return { label: 'Swap', disabled: false };
}

function hookNameOf(program: string | undefined, environment: HookEnvironment): string {
  if (program === environment.fairLaunchProgramId) return 'Fair Launch';
  if (program && program === environment.creatorCommitmentProgramId) return 'Creator Commitment';
  if (program && program === environment.holderRewardsProgramId) return 'Holder Rewards';
  return 'Unrecognised hook';
}

export interface SwapCardProps {
  environment: HookEnvironment;
  connection: Connection;
  context: PoolContext;
  balances: Balances | null;
  reload: () => Promise<PoolContext>;
  onSwapped: () => void;
  /** When the page was last loaded, for the price-freshness row. */
  loadedAt: number;
}

export function SwapCard({ environment, connection, context, balances, reload, onSwapped, loadedAt }: SwapCardProps) {
  const { connected, publicKey } = useWallet();
  const now = useNow();
  const pool = context.pool;
  const [inputIsA, setInputIsA] = useState(true);
  const [text, setText] = useState('');
  const [slippageBps, setSlippageBps] = useState<number>(50);
  const [confirmed, setConfirmed] = useState<Set<string>>(readConfirmed);

  const tokenIn = inputIsA ? pool.tokenA : pool.tokenB;
  const tokenOut = inputIsA ? pool.tokenB : pool.tokenA;
  const hooked = hookedToken(context);
  const hookedMint = hooked?.mint ?? null;
  const prefix: Record<KnownHook, string> = { 'fair-launch': 'LAUNCH', 'creator-commitment': 'VESTING', 'holder-rewards': 'REWARDS' };
  const labelOf = useCallback(
    (mint: PublicKey) =>
      hooked && hooked.mint.equals(mint) ? `${prefix[hooked.kind]} ${shortKey(mint.toBase58())}` : shortKey(mint.toBase58()),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [hookedMint, hooked?.kind]
  );

  const amountIn = useMemo(() => parseAmount(text, tokenIn.decimals), [text, tokenIn.decimals]);
  const quote = useSwapQuote(context.adapter, pool, inputIsA, amountIn, slippageBps);
  const balanceIn = balances ? (inputIsA ? balances.a : balances.b) : null;
  const balanceOut = balances ? (inputIsA ? balances.b : balances.a) : null;
  const hookedBalance = hooked && balances ? (hooked.side === 'A' ? balances.a : balances.b) : 0n;
  const view = useFairLaunch(context, inputIsA, quote, hookedBalance);

  const swap = useHookAwareSwap(connection, environment, reload, onSwapped);
  const busy = swap.state.phase !== 'idle' && swap.state.phase !== 'done';

  // hooked mints the user has not yet confirmed
  const unconfirmed = [
    { info: context.hookA, mint: pool.tokenA.mint },
    { info: context.hookB, mint: pool.tokenB.mint },
  ].filter((entry) => entry.info.hookProgramId && !confirmed.has(entry.mint.toBase58()));
  const gated = unconfirmed.length > 0;

  // a changed amount or direction invalidates an old result
  useEffect(() => {
    if (swap.state.phase === 'done') swap.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text, inputIsA]);

  const button = swapButtonState({ connected, amount: amountIn, balance: balanceIn, hasQuote: quote !== null, gated, busy });
  const outputText = quote ? toInputText(quote.amountOut, tokenOut.decimals) : '';
  const hasAnyHook = Boolean(context.hookA.hookProgramId || context.hookB.hookProgramId);
  const launchDetail = view?.phase === 'ended' ? 'Ended' : view?.phase === 'not-started' ? 'Not started' : 'Active';
  const hookLabel = hookSummary(hooked?.kind ?? null, hasAnyHook, hooked?.kind === 'fair-launch' ? launchDetail : '');
  const policyLabel =
    hooked?.kind === 'fair-launch' && context.launch
      ? `${protectionCount(context.launch.config)} protections`
      : hooked?.kind === 'creator-commitment'
        ? 'Vesting floor on the creator account'
        : hooked?.kind === 'holder-rewards'
          ? 'Pays registered holders'
          : null;
  const outcome = swap.state.phase === 'done' ? swap.state.outcome : null;
  const hookProgram =
    hooked?.kind === 'fair-launch'
      ? environment.fairLaunchProgramId
      : hooked?.kind === 'creator-commitment'
        ? (environment.creatorCommitmentProgramId ?? '')
        : hooked?.kind === 'holder-rewards'
          ? (environment.holderRewardsProgramId ?? '')
          : (context.hookA.hookProgramId ?? context.hookB.hookProgramId)?.toBase58() ?? '';

  // A sale out of the creator account is checked against the vesting floor before signing.
  const hookedDecimals = hooked ? (hooked.side === 'A' ? pool.tokenA : pool.tokenB).decimals : 0;
  const hookedProgram = hooked ? (hooked.side === 'A' ? pool.tokenA : pool.tokenB).tokenProgram : null;
  const walletHookedAccount = publicKey && hooked && hookedProgram ? getAssociatedTokenAddressSync(hooked.mint, publicKey, false, hookedProgram) : null;
  const sellingHooked = hooked ? (inputIsA ? hooked.side === 'A' : hooked.side === 'B') : false;
  const preview =
    context.commitment && walletHookedAccount && sellingHooked && amountIn !== null && amountIn > 0n
      ? previewCreatorTransfer(context.commitment.config, walletHookedAccount, hookedBalance, amountIn, now)
      : null;
  const floorCheck: FloorCheck | null = preview;
  const walletIsCreator = Boolean(context.commitment && walletHookedAccount?.equals(context.commitment.config.creatorAccount));

  return (
    <div className="trade-grid">
      <div className="stack">
        {unconfirmed.map(({ info, mint }) => (
          <UnknownTokenGate
            key={mint.toBase58()}
            mint={mint.toBase58()}
            hookName={hookNameOf(info.hookProgramId?.toBase58(), environment)}
            hookProgram={info.hookProgramId?.toBase58() ?? ''}
            onConfirm={() => {
              rememberConfirmed(mint.toBase58());
              setConfirmed(new Set(confirmed).add(mint.toBase58()));
            }}
          />
        ))}
        <section className="card swap" id="swap" aria-labelledby="swap-title">
          <h2 id="swap-title">Swap</h2>
          <TokenAmountInput
            label="From"
            tokenLabel={labelOf(tokenIn.mint)}
            value={text}
            onChange={setText}
            balance={balanceIn}
            decimals={tokenIn.decimals}
            onMax={balanceIn !== null ? () => setText(toInputText(balanceIn, tokenIn.decimals)) : undefined}
            onHalf={balanceIn !== null ? () => setText(toInputText(balanceIn / 2n, tokenIn.decimals)) : undefined}
            invalid={text !== '' && amountIn === null}
          />
          <button
            type="button"
            className="flip"
            aria-label="Switch direction"
            onClick={() => {
              setInputIsA(!inputIsA);
              setText('');
            }}
          >
            ⇅
          </button>
          <TokenAmountInput
            label="To"
            tokenLabel={labelOf(tokenOut.mint)}
            value={outputText}
            balance={balanceOut}
            decimals={tokenOut.decimals}
            readOnly
          />
          {quote && (
            <SwapSummary
              quote={quote}
              inDecimals={tokenIn.decimals}
              outDecimals={tokenOut.decimals}
              inLabel={labelOf(tokenIn.mint)}
              outLabel={labelOf(tokenOut.mint)}
              slippageBps={slippageBps}
              hookLabel={hookLabel}
              policyLabel={policyLabel}
              updatedAt={loadedAt}
            />
          )}
          <div className="slippage" role="group" aria-label="Slippage tolerance">
            {SLIPPAGE_OPTIONS_BPS.map((bps) => (
              <button
                key={bps}
                type="button"
                className={`chip${slippageBps === bps ? ' chip-on' : ''}`}
                aria-pressed={slippageBps === bps}
                onClick={() => setSlippageBps(bps)}
              >
                {bps / 100}%
              </button>
            ))}
          </div>
          {view && view.isBuy && view.violated.length > 0 && view.phase === 'active' && (
            <p className="warn-box" role="status" data-testid="preflight-warning">
              This buy breaks the Fair Launch rule{view.violated.length > 1 ? 's' : ''}: {view.violated.map((row) => row.label).join(', ')}.
              The on-chain hook will reject it.
            </p>
          )}
          {floorCheck && floorCheck.violated && (
            <p className="warn-box" role="status" data-testid="preflight-warning">
              This sale would leave the creator account below its vesting floor. The on-chain hook will reject it.
            </p>
          )}
          <button
            type="button"
            className="btn btn-primary btn-wide"
            disabled={button.disabled}
            onClick={() => amountIn !== null && void swap.run(inputIsA, amountIn, slippageBps)}
          >
            {button.label}
          </button>
          <TransactionStatus state={swap.state} environment={environment} />
          {quote && balances && (
            <p className="muted small">
              You receive at least {formatAmount(quote.minimumOut, tokenOut.decimals)} {labelOf(tokenOut.mint)}.
            </p>
          )}
        </section>
      </div>
      <div className="stack">
        {context.launch && (
          <FairLaunchPolicy
            config={context.launch.config}
            counter={context.launch.counter}
            view={view}
            decimals={(context.launch.hookedSide === 'A' ? pool.tokenA : pool.tokenB).decimals}
            hookProgramId={hookProgram}
            tokenLabel={hookedMint ? shortKey(hookedMint.toBase58()) : ''}
          />
        )}
        {context.commitment && (
          <CreatorCommitmentPolicy
            config={context.commitment.config}
            now={now}
            decimals={hookedDecimals}
            hookProgramId={hookProgram}
            tokenLabel={hookedMint ? shortKey(hookedMint.toBase58()) : ''}
            floorCheck={floorCheck}
            walletIsCreator={walletIsCreator}
          />
        )}
        {context.rewards && (
          <HolderRewardsPanel
            environment={environment}
            connection={connection}
            context={context}
            rewards={context.rewards}
            tokenLabel={hookedMint ? shortKey(hookedMint.toBase58()) : ''}
            onChanged={onSwapped}
          />
        )}
        <DeveloperDetails environment={environment} context={context} outcome={outcome} />
      </div>
    </div>
  );
}
