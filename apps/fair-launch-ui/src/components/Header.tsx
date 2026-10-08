import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { ConnectWallet } from './ConnectWallet.tsx';
import { EnvironmentBadge } from './EnvironmentBadge.tsx';
import { RaydiumMark } from './RaydiumMark.tsx';

export interface HeaderProps {
  environment: HookEnvironment;
}

export function Header({ environment }: HeaderProps) {
  return (
    <header className="header">
      <a className="brand" href="/" aria-label="Raydium Transfer Hooks, home">
        <RaydiumMark />
        <span className="brand-name">Raydium Transfer Hooks</span>
      </a>
      <div className="nav" />
      <div className="header-actions">
        <EnvironmentBadge cluster={environment.cluster} />
        <ConnectWallet cluster={environment.cluster} />
      </div>
    </header>
  );
}
