import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { getExtraAccountMetaAddress } from '@solana/spl-token';
import type { PublicKey } from '@solana/web3.js';
import { explorerUrl } from '../config.ts';
import { AddressLink } from './AddressLink.tsx';
import type { PoolContext } from '../lib/chain.ts';
import type { Balances } from '../hooks/useWalletBalances.ts';
import { formatAmount } from '../lib/amounts.ts';
import { fundCommand } from '../lib/faucet-client.ts';
import type { SwapOutcome } from '../lib/run-swap.ts';

function validationPda(mint: PublicKey, hook: PublicKey): string {
  try {
    return getExtraAccountMetaAddress(mint, hook).toBase58();
  } catch {
    return 'unavailable';
  }
}

export interface DeveloperDetailsProps {
  environment: HookEnvironment;
  context: PoolContext | null;
  outcome: SwapOutcome | null;
  /** The connected wallet, if any, and what it holds of the pool's two tokens. */
  wallet?: string | null;
  balances?: Balances | null;
  labelOf?: (mint: PublicKey) => string;
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd className="mono wrap">{children}</dd>
    </div>
  );
}

/** Collapsed by default. Shows exactly what the transaction layer did, so the page doubles as documentation. */
export function DeveloperDetails({ environment, context, outcome, wallet = null, balances = null, labelOf }: DeveloperDetailsProps) {
  const details = outcome?.details;
  const prepared = details?.prepared;
  const hooked = context
    ? [context.hookA.hookProgramId ? context.pool.tokenA.mint : null, context.hookB.hookProgramId ? context.pool.tokenB.mint : null].filter(
        (mint) => mint !== null
      )
    : [];
  const hookProgram = context?.hookA.hookProgramId ?? context?.hookB.hookProgramId ?? null;
  const slice = (slice: { pubkey: { toBase58(): string } }[] | undefined) =>
    slice && slice.length > 0 ? (
      <ol className="address-list">
        {slice.map((meta, i) => (
          <li key={`${i}-${meta.pubkey.toBase58()}`}>
            {i}: <AddressLink environment={environment} value={meta.pubkey.toBase58()} />
          </li>
        ))}
      </ol>
    ) : (
      'none (0 accounts)'
    );
  return (
    <details className="card developer" data-testid="developer-details">
      <summary>Developer details</summary>
      <dl className="kv">
        <Row label="Environment">{`${environment.name} (${environment.cluster})`}</Row>
        <Row label="Raydium program">
          <AddressLink environment={environment} value={context?.pool.programId.toBase58() ?? environment.cpmmProgramId} />
        </Row>
        {context && (
          <Row label="Pool">
            <AddressLink environment={environment} value={context.pool.poolId.toBase58()} />
          </Row>
        )}
        {hookProgram && (
          <Row label="Hook program">
            <AddressLink environment={environment} value={hookProgram.toBase58()} />
          </Row>
        )}
        {hooked[0] && (
          <Row label="Hooked mint">
            <AddressLink environment={environment} kind="token" value={hooked[0].toBase58()} />
          </Row>
        )}
        {hooked[0] && hookProgram && (
          <Row label="Validation PDA">
            <AddressLink environment={environment} value={validationPda(hooked[0], hookProgram)} />
          </Row>
        )}
        <Row label={`Input hook accounts (${prepared?.input.slice.length ?? '-'})`}>{slice(prepared?.input.slice)}</Row>
        <Row label={`Output hook accounts (${prepared?.output.slice.length ?? '-'})`}>{slice(prepared?.output.slice)}</Row>
        <Row label="Instruction">
          {prepared
            ? `${prepared.kind.toUpperCase()}  ${prepared.label}`
            : context
              ? `${context.pool.kind.toUpperCase()}  ${context.adapter.instruction}`
              : '—'}
        </Row>
        <Row label="Your tokens">{yourTokens(environment, context, wallet, balances, labelOf)}</Row>
        <Row label="Transaction version">v0</Row>
        <Row label="Simulation compute">
          {details?.simulation?.unitsConsumed != null ? `${details.simulation.unitsConsumed} units` : '—'}
        </Row>
        {outcome?.status === 'success' && (
          <>
            <Row label="Signature">{outcome.signature}</Row>
            <Row label="Explorer">
              <a href={explorerUrl(environment, 'tx', outcome.signature)} target="_blank" rel="noreferrer">
                open
              </a>
            </Row>
          </>
        )}
        {outcome?.status === 'blocked' && <Row label="Raw error">{outcome.failure.raw}</Row>}
      </dl>
    </details>
  );
}

/** What the page knows about the wallet before anything is clicked, and what to do when it holds nothing. */
function yourTokens(
  environment: HookEnvironment,
  context: PoolContext | null,
  wallet: string | null,
  balances: Balances | null,
  labelOf?: (mint: PublicKey) => string
): string {
  if (!context) return '—';
  if (!wallet) return 'No wallet connected, so no balances. Connect one to see what it holds of this pool’s two tokens.';
  if (!balances) return 'Reading this wallet’s balances…';
  const { tokenA, tokenB } = context.pool;
  const label = (mint: PublicKey) => (labelOf ? labelOf(mint) : `${mint.toBase58().slice(0, 4)}…${mint.toBase58().slice(-4)}`);
  const held = `${formatAmount(balances.a, tokenA.decimals)} ${label(tokenA.mint)} and ${formatAmount(balances.b, tokenB.decimals)} ${label(tokenB.mint)}`;
  if (balances.a === 0n && balances.b === 0n) {
    return `${held}. This wallet holds none of this pool’s tokens, so there is nothing to swap yet. Click “Get test tokens” (it mints 100 of each from the dev server’s faucet), or run: ${fundCommand(environment, wallet, context.pool.poolId.toBase58())}`;
  }
  return held;
}
