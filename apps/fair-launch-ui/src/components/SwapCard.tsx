import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { useWallet } from '@solana/wallet-adapter-react';
import type { Connection, PublicKey } from '@solana/web3.js';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useFairLaunch } from '../hooks/useFairLaunch.ts';
import { useHookAwareSwap } from '../hooks/useHookAwareSwap.ts';
import { useSwapQuote } from '../hooks/useSwapQuote.ts';
import type { Balances } from '../hooks/useWalletBalances.ts';
import { formatAmount, parseAmount, shortKey, toInputText } from '../lib/amounts.ts';
import type { PoolContext } from '../lib/chain.ts';
import { DeveloperDetails } from './DeveloperDetails.tsx';
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
  const { connected } = useWallet();
  const pool = context.pool;
  const [inputIsA, setInputIsA] = useState(true);
  const [text, setText] = useState('');
  const [slippageBps, setSlippageBps] = useState<number>(50);
  const [confirmed, setConfirmed] = useState<Set<string>>(readConfirmed);

  const tokenIn = inputIsA ? pool.tokenA : pool.tokenB;
  const tokenOut = inputIsA ? pool.tokenB : pool.tokenA;
  const launchMint = context.launch ? (context.launch.hookedSide === 'A' ? pool.tokenA.mint : pool.tokenB.mint) : null;
  const labelOf = useCallback(
    (mint: PublicKey) =>
      launchMint?.equals(mint) ? `LAUNCH ${shortKey(mint.toBase58())}` : shortKey(mint.toBase58()),
    [launchMint]
  );

  const amountIn = useMemo(() => parseAmount(text, tokenIn.decimals), [text, tokenIn.decimals]);
  const quote = useSwapQuote(context.adapter, pool, inputIsA, amountIn, slippageBps);
  const balanceIn = balances ? (inputIsA ? balances.a : balances.b) : null;
  const balanceOut = balances ? (inputIsA ? balances.b : balances.a) : null;
  const hookedBalance = context.launch && balances ? (context.launch.hookedSide === 'A' ? balances.a : balances.b) : 0n;
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
  const hookLabel = context.launch
    ? `Fair Launch · ${view?.phase === 'ended' ? 'Ended' : view?.phase === 'not-started' ? 'Not started' : 'Active'}`
    : context.hookA.hookProgramId || context.hookB.hookProgramId
      ? 'Unrecognised hook'
      : 'None';
  const outcome = swap.state.phase === 'done' ? swap.state.outcome : null;
  const hookProgram = context.launch ? environment.fairLaunchProgramId : (context.hookA.hookProgramId ?? context.hookB.hookProgramId)?.toBase58() ?? '';

  return (
    <div className="trade-grid">
      <div className="stack">
        {unconfirmed.map(({ info, mint }) => (
          <UnknownTokenGate
            key={mint.toBase58()}
            mint={mint.toBase58()}
            hookName={info.hookProgramId?.toBase58() === environment.fairLaunchProgramId ? 'Fair Launch' : 'Unrecognised hook'}
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
              policyLabel={context.launch ? `${protectionCount(context.launch.config)} protections` : null}
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
            tokenLabel={launchMint ? shortKey(launchMint.toBase58()) : ''}
          />
        )}
        <DeveloperDetails environment={environment} context={context} outcome={outcome} />
      </div>
    </div>
  );
}
