import type { HookEnvironment } from '@raydium-transfer-hook/client';
import type { ReactNode } from 'react';
import { type SolscanKind, solscanUrl } from '../lib/solscan.ts';

export interface AddressLinkProps {
  /** Without an environment (a test that renders a panel alone) the value is shown as plain text. */
  environment?: HookEnvironment;
  /** `token` for a mint, `account` for anything else: a program, a pool, a token account, a wallet. */
  kind?: SolscanKind;
  value: string;
  /** What to show instead of the address, e.g. a shortened one. */
  label?: ReactNode;
  className?: string;
}

/** Any token, program or account on the page opens on Solscan, on the page's own cluster. */
export function AddressLink({ environment, kind = 'account', value, label, className = 'mono' }: AddressLinkProps) {
  if (!environment || !value) return <>{label ?? value}</>;
  return (
    <a href={solscanUrl(environment, kind, value)} target="_blank" rel="noreferrer" className={className} title={`Open ${value} on Solscan`}>
      {label ?? value}
    </a>
  );
}
