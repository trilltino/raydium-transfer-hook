import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { useWallet } from '@solana/wallet-adapter-react';
import { shortKey } from '../lib/amounts.ts';
import { EnvironmentBadge } from './EnvironmentBadge.tsx';

const DOCS_URL = 'https://github.com/trilltino/raydium-transfer-hook/blob/main/docs/frontend.md';
const REPO_URL = 'https://github.com/trilltino/raydium-transfer-hook';

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
  environments: readonly HookEnvironment[];
  onEnvironmentChange: (name: string) => void;
}

export function Header({ environment, environments, onEnvironmentChange }: HeaderProps) {
  return (
    <header className="header">
      <div className="brand">
        <span className="brand-mark" aria-hidden="true" />
        <span className="brand-name">Raydium Transfer Hooks</span>
      </div>
      <nav className="nav" aria-label="Primary">
        <a href="#swap" aria-current="page">
          Swap
        </a>
        <a href={DOCS_URL} target="_blank" rel="noreferrer">
          Docs
        </a>
        <a href={REPO_URL} target="_blank" rel="noreferrer">
          GitHub
        </a>
      </nav>
      <div className="header-actions">
        <label className="visually-hidden" htmlFor="environment">
          Environment
        </label>
        <select
          id="environment"
          className="select"
          value={environment.name}
          onChange={(event) => onEnvironmentChange(event.target.value)}
        >
          {environments.map((entry) => (
            <option key={entry.name} value={entry.name}>
              {entry.name}
            </option>
          ))}
        </select>
        <EnvironmentBadge name={environment.name} />
        <WalletButton />
      </div>
    </header>
  );
}
