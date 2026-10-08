import './polyfills.ts';
import { ConnectionProvider, WalletProvider } from '@solana/wallet-adapter-react';
import { PhantomWalletAdapter } from '@solana/wallet-adapter-phantom';
import { SolflareWalletAdapter } from '@solana/wallet-adapter-solflare';
import type { Adapter } from '@solana/wallet-adapter-base';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App.tsx';
import { E2E_BUILD, readSelection } from './config.ts';
import './styles.css';

async function wallets(): Promise<Adapter[]> {
  const list: Adapter[] = [new PhantomWalletAdapter(), new SolflareWalletAdapter()];
  // The browser test wallet exists only in builds made with VITE_E2E=1; the condition is replaced at
  // build time, so a normal build contains neither this branch nor the adapter.
  if (E2E_BUILD) {
    const secret = new URLSearchParams(window.location.search).get('testWallet');
    if (secret) {
      const { TestWalletAdapter } = await import('./testing/test-wallet.ts');
      list.unshift(new TestWalletAdapter(Uint8Array.from(JSON.parse(secret) as number[])));
    }
  }
  return list;
}

const rootElement = document.getElementById('root');
if (!rootElement) throw new Error('missing #root');

void wallets().then((adapters) => {
  const { environment } = readSelection(window.location.search);
  createRoot(rootElement).render(
    <StrictMode>
      <ConnectionProvider endpoint={environment.rpcUrl}>
        <WalletProvider wallets={adapters} autoConnect={E2E_BUILD}>
          <App />
        </WalletProvider>
      </ConnectionProvider>
    </StrictMode>
  );
});
