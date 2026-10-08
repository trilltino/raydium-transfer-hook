import type { HookCluster } from '@raydium-transfer-hook/client';

/** Which network the page is on. The page is for devnet; a local validator is only reached by `?env=localnet` (the browser tests). */
export function EnvironmentBadge({ cluster }: { cluster: HookCluster }) {
  return <span className="badge badge-env">{cluster === 'devnet' ? 'Devnet' : 'Local validator'}</span>;
}
