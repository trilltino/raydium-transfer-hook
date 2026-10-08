import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { useWallet } from '@solana/wallet-adapter-react';
import { shortKey } from '../lib/amounts.ts';
import { EnvironmentBadge } from './EnvironmentBadge.tsx';
import { RaydiumMark } from './RaydiumMark.tsx';

export function WalletButton() {
  const { wallets, wallet, publicKey, connected, connecting, select, connect, disconnect } = useWallet();
  if (connected && publicKey) {
    return (
      <button type="button" className="btn btn-ghost" onClick={() => void disconnect()} title={publicKey.toBase58()}>
        {shortKey(publicKey.toBase58())} · Disconnect
      </button>
    );
  }
  if (wallet) {
    return (
      <button type="button" className="btn btn-primary" disabled={connecting} onClick={() => void connect().catch(() => undefined)}>
        {connecting ? 'Connecting…' : `Connect ${wallet.adapter.name}`}
      </button>
    );
  }
  const usable = wallets.filter((entry) => entry.readyState !== 'Unsupported');
  return (
    <div className="wallet-list" role="group" aria-label="Choose a wallet">
      {usable.length === 0 && <span className="muted">No wallet found</span>}
      {usable.map((entry) => (
        <button key={entry.adapter.name} type="button" className="btn btn-primary" onClick={() => select(entry.adapter.name)}>
          Connect {entry.adapter.name}
        </button>
      ))}
    </div>
  );
}

export interface HeaderProps {
  environment: HookEnvironment;
}

export function Header({ environment }: HeaderProps) {
  return (
    <header className="header">
      <div className="brand">
        <RaydiumMark />
        <span className="brand-name">Raydium Transfer Hooks</span>
      </div>
      <div className="nav" />
      <div className="header-actions">
        <EnvironmentBadge cluster={environment.cluster} />
        <WalletButton />
      </div>
    </header>
  );
}
