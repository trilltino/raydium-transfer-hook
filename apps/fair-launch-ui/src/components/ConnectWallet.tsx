import type { HookCluster } from '@raydium-transfer-hook/client';
import { WalletReadyState } from '@solana/wallet-adapter-base';
import { useWallet } from '@solana/wallet-adapter-react';
import { useEffect, useState } from 'react';
import { shortKey } from '../lib/amounts.ts';

/** First the wallets the browser actually has (Phantom, Solflare and any other Wallet Standard extension), then the rest. */
const rank = (state: WalletReadyState): number => (state === WalletReadyState.Installed ? 0 : state === WalletReadyState.Loadable ? 1 : 2);

/**
 * "Connect wallet": a dialog that lists the wallets with their own logos. A wallet the browser has is one click
 * (select and connect together); one it does not have opens its install page. Wallets that register themselves
 * with the Wallet Standard (Solflare, Phantom, Backpack…) show up without being listed in the code.
 */
export function ConnectWallet({ cluster }: { cluster: HookCluster }) {
  const { wallets, wallet, publicKey, connected, connecting, select, connect, disconnect } = useWallet();
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // `select` takes effect on the next render; connect once the chosen wallet is the selected one. This runs in a timer, not
  // in the effect itself: effects of a child run before the wallet provider's, which has not yet started listening to the
  // newly selected wallet, so a connection made right here would finish unnoticed and the page would never see it.
  useEffect(() => {
    if (!pending || wallet?.adapter.name !== pending || connected || connecting) return;
    const timer = setTimeout(() => {
      setPending(null);
      connect()
        .then(() => setOpen(false))
        .catch((reason: unknown) => setError(reason instanceof Error && reason.message ? reason.message : 'The wallet did not connect.'));
    }, 0);
    return () => clearTimeout(timer);
  }, [pending, wallet, connected, connecting, connect]);

  useEffect(() => {
    if (!open) return;
    const close = (event: KeyboardEvent) => event.key === 'Escape' && setOpen(false);
    window.addEventListener('keydown', close);
    return () => window.removeEventListener('keydown', close);
  }, [open]);

  if (connected && publicKey) {
    return (
      <button type="button" className="btn btn-ghost" onClick={() => void disconnect()} title={publicKey.toBase58()}>
        {wallet && <img className="wallet-logo wallet-logo-small" src={wallet.adapter.icon} alt="" />}
        {shortKey(publicKey.toBase58())} · Disconnect
      </button>
    );
  }

  const usable = wallets.filter((entry) => entry.readyState !== WalletReadyState.Unsupported).sort((a, b) => rank(a.readyState) - rank(b.readyState));
  const choose = (name: (typeof usable)[number]['adapter']['name'], state: WalletReadyState, url: string) => {
    setError(null);
    if (state === WalletReadyState.NotDetected) {
      window.open(url, '_blank', 'noreferrer');
      return;
    }
    setPending(name);
    if (wallet?.adapter.name === name) return; // already selected: the effect connects
    select(name);
  };

  return (
    <>
      <button type="button" className="btn btn-primary" disabled={connecting} onClick={() => setOpen(true)}>
        {connecting ? 'Connecting…' : 'Connect wallet'}
      </button>
      {open && (
        <div className="modal-backdrop" onClick={() => setOpen(false)} role="presentation">
          <div className="modal" role="dialog" aria-modal="true" aria-labelledby="wallet-title" onClick={(event) => event.stopPropagation()}>
            <div className="modal-head">
              <h2 id="wallet-title">Connect a wallet</h2>
              <button type="button" className="btn btn-ghost modal-close" aria-label="Close" onClick={() => setOpen(false)}>
                ×
              </button>
            </div>
            <p className="muted small">
              {cluster === 'devnet'
                ? 'Set the wallet to Devnet first (in its settings), or the page will find no SOL.'
                : 'Point the wallet at the local validator, http://127.0.0.1:8899, first.'}
            </p>
            {usable.length === 0 && <p className="muted">No wallet found in this browser.</p>}
            <ul className="wallet-options">
              {usable.map((entry) => {
                const detected = entry.readyState !== WalletReadyState.NotDetected;
                return (
                  <li key={entry.adapter.name}>
                    <button type="button" className="wallet-option" onClick={() => choose(entry.adapter.name, entry.readyState, entry.adapter.url)}>
                      <img className="wallet-logo" src={entry.adapter.icon} alt="" />
                      <span className="wallet-name">{entry.adapter.name}</span>
                      <span className={detected ? 'wallet-state wallet-detected' : 'wallet-state'}>{detected ? 'Detected' : 'Install'}</span>
                    </button>
                  </li>
                );
              })}
            </ul>
            {error && (
              <p className="notice notice-error" role="alert" data-testid="wallet-error">
                {error}
              </p>
            )}
          </div>
        </div>
      )}
    </>
  );
}
