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
      <div className="brand">
        <RaydiumMark />
        <span className="brand-name">Raydium Transfer Hooks</span>
      </div>
      <div className="nav" />
      <div className="header-actions">
        <EnvironmentBadge cluster={environment.cluster} />
        <ConnectWallet cluster={environment.cluster} />
      </div>
    </header>
  );
}
