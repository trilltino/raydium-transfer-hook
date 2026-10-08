import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { WalletReadyState } from '@solana/wallet-adapter-base';
import { Keypair } from '@solana/web3.js';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { poolContext } from './fixtures.ts';

type Entry = { adapter: { name: string; icon: string; url: string }; readyState: WalletReadyState };
const wallet = vi.hoisted(() => ({
  wallets: [] as Entry[],
  wallet: null as Entry | null,
  publicKey: null as unknown,
  connected: false,
  connecting: false,
  select: vi.fn(),
  connect: vi.fn(async () => undefined),
  disconnect: vi.fn(async () => undefined),
}));
vi.mock('@solana/wallet-adapter-react', () => ({ useWallet: () => wallet }));

import { AddressLink } from '../src/components/AddressLink.tsx';
import { ConnectWallet } from '../src/components/ConnectWallet.tsx';
import { DeveloperDetails } from '../src/components/DeveloperDetails.tsx';
import { FairLaunchPolicy } from '../src/components/FairLaunchPolicy.tsx';
import { TokenAmountInput } from '../src/components/TokenAmountInput.tsx';

const devnet: HookEnvironment = {
  name: 'integration-devnet',
  cluster: 'devnet',
  rpcUrl: 'https://api.devnet.solana.com',
  cpmmProgramId: '7tRJH4mmEfNGGLf9E8qEvo3oSjq2b75DhSggb1Wz45fJ',
  clmmProgramId: '3dNJapViueBArDN3HbWKcEf2u6hfZQ3oDyh3GmUXJ8oD',
  fairLaunchProgramId: '7xyk1AQg7xCaQucPgWs13hmAs4dmD214raNgEZhLPQSu',
};
const entry = (name: string, readyState: WalletReadyState): Entry => ({ adapter: { name, icon: `data:image/svg+xml;base64,${name}`, url: `https://${name.toLowerCase()}.app` }, readyState });

beforeEach(() => {
  wallet.wallets = [entry('Phantom', WalletReadyState.NotDetected), entry('Solflare', WalletReadyState.Installed), entry('Hidden', WalletReadyState.Unsupported)];
  wallet.wallet = null;
  wallet.publicKey = null;
  wallet.connected = false;
  wallet.connecting = false;
  wallet.select.mockReset();
  wallet.connect.mockReset().mockResolvedValue(undefined);
});

describe('the wallet dialog', () => {
  it('opens from one button and lists the wallets with their own logos, the ones the browser has first', () => {
    render(<ConnectWallet cluster="devnet" />);
    expect(screen.queryByRole('dialog')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Connect wallet' }));
    const dialog = screen.getByRole('dialog');
    const options = within(dialog).getAllByRole('button').filter((button) => button.className.includes('wallet-option'));
    expect(options.map((option) => option.textContent)).toEqual(['SolflareDetected', 'PhantomInstall']);
    expect(options[0].querySelector('img')?.getAttribute('src')).toBe('data:image/svg+xml;base64,Solflare');
    expect(within(dialog).queryByText('Hidden')).toBeNull();
    expect(dialog.textContent).toContain('Set the wallet to Devnet first');
  });

  it('selects and connects a detected wallet in one click, then closes', async () => {
    const { rerender } = render(<ConnectWallet cluster="devnet" />);
    fireEvent.click(screen.getByRole('button', { name: 'Connect wallet' }));
    fireEvent.click(screen.getByRole('button', { name: /Solflare/ }));
    expect(wallet.select).toHaveBeenCalledWith('Solflare');
    // the provider makes it the selected wallet on its next render; the dialog then connects
    wallet.wallet = wallet.wallets[1];
    rerender(<ConnectWallet cluster="devnet" />);
    await waitFor(() => expect(wallet.connect).toHaveBeenCalledOnce());
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('sends a wallet the browser does not have to its install page instead of trying to connect', () => {
    const open = vi.spyOn(window, 'open').mockImplementation(() => null);
    render(<ConnectWallet cluster="devnet" />);
    fireEvent.click(screen.getByRole('button', { name: 'Connect wallet' }));
    fireEvent.click(screen.getByRole('button', { name: /Phantom/ }));
    expect(open).toHaveBeenCalledWith('https://phantom.app', '_blank', 'noreferrer');
    expect(wallet.select).not.toHaveBeenCalled();
    open.mockRestore();
  });

  it('shows the wallet’s own message when it refuses, and closes on Escape', async () => {
    wallet.wallet = wallet.wallets[1];
    wallet.connect.mockRejectedValue(new Error('User rejected the request.'));
    render(<ConnectWallet cluster="localnet" />);
    fireEvent.click(screen.getByRole('button', { name: 'Connect wallet' }));
    fireEvent.click(screen.getByRole('button', { name: /Solflare/ }));
    expect((await screen.findByTestId('wallet-error')).textContent).toBe('User rejected the request.');
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('once connected shows the wallet’s logo and a short address with Disconnect', () => {
    wallet.connected = true;
    wallet.wallet = wallet.wallets[1];
    wallet.publicKey = Keypair.fromSeed(Buffer.alloc(32, 3)).publicKey;
    render(<ConnectWallet cluster="devnet" />);
    const button = screen.getByRole('button', { name: /Disconnect/ });
    expect(button.querySelector('img')?.getAttribute('src')).toContain('Solflare');
    fireEvent.click(button);
    expect(wallet.disconnect).toHaveBeenCalledOnce();
  });
});

describe('every token and program links to Solscan on the page’s cluster', () => {
  const hrefs = (container: HTMLElement) => [...container.querySelectorAll('a')].map((a) => a.getAttribute('href'));

  it('an address link points at the account page, a mint at the token page, and is plain text without an environment', () => {
    const { container } = render(
      <>
        <AddressLink environment={devnet} value="PROGRAM" />
        <AddressLink environment={devnet} kind="token" value="MINT" label="LAUNCH" />
        <AddressLink value="PLAIN" />
      </>
    );
    expect(hrefs(container)).toEqual(['https://solscan.io/account/PROGRAM?cluster=devnet', 'https://solscan.io/token/MINT?cluster=devnet']);
    expect(container.textContent).toBe('PROGRAMLAUNCHPLAIN');
  });

  it('the token chip of the swap box links to the mint', () => {
    const { container } = render(<TokenAmountInput label="From" tokenLabel="FiEb…jyh2" value="1" decimals={6} href="https://solscan.io/token/M?cluster=devnet" />);
    expect(container.querySelector('a.token-chip')?.getAttribute('href')).toBe('https://solscan.io/token/M?cluster=devnet');
  });

  it('the Fair Launch panel links its token and hook program', () => {
    const config = poolContext().launch!.config;
    const { container } = render(
      <FairLaunchPolicy environment={devnet} tokenMint="MINT" config={config} counter={null} view={null} decimals={6} hookProgramId="HOOK" tokenLabel="LAUNCH" />
    );
    expect(hrefs(container)).toEqual(['https://solscan.io/token/MINT?cluster=devnet', 'https://solscan.io/account/HOOK?cluster=devnet']);
  });

  it('Developer details links the programs, the pool, the mint and each hook account', () => {
    const context = poolContext();
    const { container } = render(<DeveloperDetails environment={devnet} context={context} outcome={null} />);
    const links = hrefs(container);
    expect(links).toContain(`https://solscan.io/account/${context.pool.poolId.toBase58()}?cluster=devnet`);
    expect(links).toContain(`https://solscan.io/token/${context.pool.tokenA.mint.toBase58()}?cluster=devnet`);
    expect(links.every((href) => href?.startsWith('https://solscan.io/') && href.endsWith('cluster=devnet'))).toBe(true);
  });
});

describe('the network the wallets are told about', () => {
  it('is devnet, so Solflare does not refuse to sign with "this transaction is for mainnet"', async () => {
    const { networkFor, walletAdapters } = await import('../src/lib/wallets.ts');
    const { WalletAdapterNetwork } = await import('@solana/wallet-adapter-base');
    expect(networkFor(devnet)).toBe(WalletAdapterNetwork.Devnet);
    expect(networkFor({ ...devnet, name: 'localnet', cluster: 'localnet' })).toBe(WalletAdapterNetwork.Devnet);
    expect(walletAdapters(devnet).map((adapter) => adapter.name)).toEqual(['Phantom', 'Solflare']);
  });
});
