import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { getExtraAccountMetaAddress } from '@solana/spl-token';
import type { PublicKey } from '@solana/web3.js';
import { explorerUrl } from '../config.ts';
import type { PoolContext } from '../lib/chain.ts';
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
export function DeveloperDetails({ environment, context, outcome }: DeveloperDetailsProps) {
  const details = outcome?.details;
  const prepared = details?.prepared;
  const hooked = context
    ? [context.hookA.hookProgramId ? context.pool.tokenA.mint : null, context.hookB.hookProgramId ? context.pool.tokenB.mint : null].filter(
        (mint) => mint !== null
      )
    : [];
  const hookProgram = context?.hookA.hookProgramId ?? context?.hookB.hookProgramId ?? null;
  const slice = (slice: { pubkey: { toBase58(): string } }[] | undefined) =>
    slice && slice.length > 0 ? slice.map((meta, i) => `${i}: ${meta.pubkey.toBase58()}`).join('\n') : 'none (0 accounts)';
  return (
    <details className="card developer" data-testid="developer-details">
      <summary>Developer details</summary>
      <dl className="kv">
        <Row label="Environment">{`${environment.name} (${environment.cluster})`}</Row>
        <Row label="Raydium program">{context?.pool.programId.toBase58() ?? environment.cpmmProgramId}</Row>
        {context && <Row label="Pool">{context.pool.poolId.toBase58()}</Row>}
        {hookProgram && <Row label="Hook program">{hookProgram.toBase58()}</Row>}
        {hooked[0] && <Row label="Hooked mint">{hooked[0].toBase58()}</Row>}
        {hooked[0] && hookProgram && <Row label="Validation PDA">{validationPda(hooked[0], hookProgram)}</Row>}
        <Row label={`Input hook accounts (${prepared?.input.slice.length ?? '-'})`}>{slice(prepared?.input.slice)}</Row>
        <Row label={`Output hook accounts (${prepared?.output.slice.length ?? '-'})`}>{slice(prepared?.output.slice)}</Row>
        <Row label="Instruction">
          {prepared
            ? `${prepared.kind.toUpperCase()}  ${prepared.label}`
            : context
              ? `${context.pool.kind.toUpperCase()}  ${context.adapter.instruction}`
              : '—'}
        </Row>
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
