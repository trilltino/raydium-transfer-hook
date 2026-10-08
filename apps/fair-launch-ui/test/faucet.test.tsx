import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { type Connection, Keypair } from '@solana/web3.js';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { poolContext } from './fixtures.ts';

const wallet = vi.hoisted(() => ({ signTransaction: vi.fn(async (tx: unknown) => tx) }));
vi.mock('@solana/wallet-adapter-react', () => ({ useWallet: () => wallet }));
const runTx = vi.hoisted(() => vi.fn());
vi.mock('../src/lib/run-transaction.ts', () => ({ runTransaction: runTx }));
const authority = vi.hoisted(() => ({ own: new Set<string>() }));
vi.mock('../src/lib/mint-authority.ts', () => ({ mintedByWallet: async () => authority.own }));

import { DeveloperDetails } from '../src/components/DeveloperDetails.tsx';
import { FairLaunchPolicy } from '../src/components/FairLaunchPolicy.tsx';
import { TestTokens } from '../src/components/TestTokens.tsx';
import { faucetAvailable, fundCommand, requestTokens } from '../src/lib/faucet-client.ts';
import { presentContext, presentSimulationFailure } from '../src/lib/present.ts';

const devnet: HookEnvironment = {
  name: 'integration-devnet',
  cluster: 'devnet',
  rpcUrl: 'https://api.devnet.solana.com',
  cpmmProgramId: '7tRJH4mmEfNGGLf9E8qEvo3oSjq2b75DhSggb1Wz45fJ',
  clmmProgramId: '3dNJapViueBArDN3HbWKcEf2u6hfZQ3oDyh3GmUXJ8oD',
  fairLaunchProgramId: '7xyk1AQg7xCaQucPgWs13hmAs4dmD214raNgEZhLPQSu',
};
const walletKey = Keypair.fromSeed(Buffer.alloc(32, 9)).publicKey;
const WALLET = walletKey.toBase58();
const context = poolContext();
const tokens = [context.pool.tokenA, context.pool.tokenB];
const connection = {} as Connection;
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
const asFetch = (fn: (url: unknown, init?: RequestInit) => Promise<Response>) => fn as unknown as typeof fetch;

beforeEach(() => {
  authority.own = new Set();
  runTx.mockReset();
});
afterEach(() => vi.unstubAllGlobals());

describe('the faucet client', () => {
  it('asks the dev server whether it can mint for this cluster', async () => {
    expect(await faucetAvailable(devnet, asFetch(async () => json({ devnet: true, localnet: false })))).toBe(true);
    expect(await faucetAvailable(devnet, asFetch(async () => json({ devnet: false, localnet: true })))).toBe(false);
    expect(await faucetAvailable(devnet, asFetch(async () => json({}, 404)))).toBe(false);
    expect(
      await faucetAvailable(
        devnet,
        asFetch(async () => {
          throw new TypeError('no server');
        })
      )
    ).toBe(false);
  });

  it('posts the wallet and mints, and turns an error answer into its message', async () => {
    const seen: RequestInit[] = [];
    const answer = await requestTokens(
      devnet,
      WALLET,
      ['M'],
      100,
      asFetch(async (_url, init) => {
        seen.push(init!);
        return json({ signature: 'SIG', results: [{ mint: 'M', status: 'minted' }] });
      })
    );
    expect(answer.signature).toBe('SIG');
    expect(JSON.parse(String(seen[0].body))).toEqual({ cluster: 'devnet', wallet: WALLET, mints: ['M'], tokens: 100 });
    await expect(requestTokens(devnet, WALLET, ['M'], 100, asFetch(async () => json({ error: 'wait a few seconds before asking again' }, 429)))).rejects.toThrow('wait a few seconds');
  });

  it('prints the terminal command for the same thing', () => {
    expect(fundCommand(devnet, WALLET, 'POOL')).toBe(`npm --workspace apps/fair-launch-ui run fund -- ${WALLET} --pool POOL --cluster devnet`);
  });
});

describe('getting tokens on the page', () => {
  const render_ = (onFunded = vi.fn()) => {
    render(<TestTokens environment={devnet} connection={connection} wallet={walletKey} tokens={tokens} onFunded={onFunded} />);
    return onFunded;
  };

  it('shows nothing when the wallet is not a mint authority and the dev server has no faucet', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => json({ devnet: false, localnet: false })));
    render_();
    await waitFor(() => expect(vi.mocked(fetch)).toHaveBeenCalled());
    expect(screen.queryByTestId('test-tokens')).toBeNull();
  });

  it('lets the creator mint their own token from their own wallet: simulated, signed by the wallet, no server involved', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => json({ devnet: false })));
    authority.own = new Set([tokens[0].mint.toBase58()]);
    runTx.mockResolvedValue({ status: 'success', signature: 'WALLETSIG', slot: 1, simulation: {} });
    const funded = render_();
    fireEvent.click(await screen.findByRole('button', { name: 'Mint 100 to my wallet' }));
    const status = await screen.findByTestId('test-tokens-own-status');
    expect(status.textContent).toContain('Minted 100 of 1 token you are the mint authority of');
    expect(status.querySelector('a')?.getAttribute('href')).toBe('https://solscan.io/tx/WALLETSIG?cluster=devnet');
    expect(funded).toHaveBeenCalledOnce();
    // one instruction pair (the token account, the mint) for the one token, signed through the wallet
    const request = runTx.mock.calls[0][0];
    expect(request.instructions).toHaveLength(2);
    expect(request.payer.equals(walletKey)).toBe(true);
    expect(request.signTransaction).toBe(wallet.signTransaction);
    expect(screen.queryByRole('button', { name: 'Get test tokens' })).toBeNull();
  });

  it('says why when the mint is refused, and reloads nothing', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => json({ devnet: false })));
    authority.own = new Set(tokens.map((token) => token.mint.toBase58()));
    runTx.mockResolvedValue({ status: 'blocked', failure: { title: 'Your wallet has no SOL on this network', reason: 'Fund it.' } });
    const funded = render_();
    fireEvent.click(await screen.findByRole('button', { name: 'Mint 100 to my wallet' }));
    expect((await screen.findByTestId('test-tokens-own-error')).textContent).toContain('Your wallet has no SOL on this network');
    expect(funded).not.toHaveBeenCalled();
  });

  it('offers the dev server faucet for demo tokens, and says what it minted with a Solscan link', async () => {
    const calls: string[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: unknown, init?: RequestInit) => {
        calls.push(init?.method ?? 'GET');
        return init?.method === 'POST'
          ? json({ signature: 'SIGNATURE', results: [{ mint: 'A', status: 'minted' }, { mint: 'B', status: 'minted' }] })
          : json({ devnet: true });
      })
    );
    const funded = render_();
    fireEvent.click(await screen.findByRole('button', { name: 'Get test tokens' }));
    const status = await screen.findByTestId('test-tokens-status');
    expect(status.textContent).toContain('Minted 100 of 2 tokens');
    expect(status.querySelector('a')?.getAttribute('href')).toBe('https://solscan.io/tx/SIGNATURE?cluster=devnet');
    expect(funded).toHaveBeenCalledOnce();
    expect(calls).toEqual(['GET', 'POST']);
  });

  it('shows why when the faucet could mint nothing, and does not reload', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: unknown, init?: RequestInit) =>
        init?.method === 'POST'
          ? json({ signature: null, results: [{ mint: 'A', status: 'skipped', reason: 'its mint authority has been given up' }] })
          : json({ devnet: true })
      )
    );
    const funded = render_();
    fireEvent.click(await screen.findByRole('button', { name: 'Get test tokens' }));
    expect((await screen.findByTestId('test-tokens-status')).textContent).toContain('Nothing minted: its mint authority has been given up');
    expect(funded).not.toHaveBeenCalled();
  });
});

describe('Developer details, before anything is clicked', () => {
  const row = () => screen.getByText('Your tokens').nextElementSibling!.textContent!;

  it('says there are no balances without a wallet', () => {
    render(<DeveloperDetails environment={devnet} context={context} outcome={null} />);
    expect(row()).toContain('No wallet connected');
  });

  it('explains a wallet that holds nothing, and what to do about it', () => {
    render(<DeveloperDetails environment={devnet} context={context} outcome={null} wallet={WALLET} balances={{ a: 0n, b: 0n }} />);
    expect(row()).toContain('holds none of this pool’s tokens');
    expect(row()).toContain('Get test tokens');
    expect(row()).toContain(`run fund -- ${WALLET} --pool ${context.pool.poolId.toBase58()} --cluster devnet`);
  });

  it('just lists the balances once the wallet holds something', () => {
    render(<DeveloperDetails environment={devnet} context={context} outcome={null} wallet={WALLET} balances={{ a: 100_000_000n, b: 5_000_000n }} />);
    expect(row()).toContain('100');
    expect(row()).not.toContain('none of this pool');
  });
});

describe('a wallet with no SOL', () => {
  it('is told so in words instead of "AccountNotFound"', () => {
    const view = presentSimulationFailure({ kind: 'other', message: 'AccountNotFound' }, presentContext(devnet));
    expect(view.title).toBe('Your wallet has no SOL on this network');
    expect(view.reason).toContain('set to the same network as this page (Devnet)');
    expect(view.notSubmitted).toBe(true);
    expect(view.raw).toContain('AccountNotFound');
  });
});

describe('the Fair Launch panel says what the launch enforces', () => {
  const config = { ...poolContext().launch!.config, maxBuy: 100_000_000n, maxWallet: 300_000_000n, maxBuysPerSlot: 3, maxPriorityMicroLamports: 1000n };
  const panel = (onTry?: (kind: 'allowed' | 'over-limit') => void) =>
    render(<FairLaunchPolicy config={config} counter={null} view={null} decimals={6} hookProgramId="HOOK" tokenLabel="LAUNCH" onTry={onTry} />);

  it('lists each switched-on rule in plain words, anti-bundle and anti-snipe included', () => {
    panel();
    const text = screen.getByTestId('policy-rules').textContent!;
    expect(text).toContain('No single buy may take more than 100 LAUNCH');
    expect(text).toContain('No wallet may hold more than 300 LAUNCH after a buy');
    expect(text).toContain('At most 3 buys per slot, across every pool of this token: a bundle with more is refused as a whole');
    expect(text).toContain('priority fee above 1000 µ-lamports');
    expect(text).toContain('Selling is never restricted');
  });

  it('can fill the swap box with a buy inside the limits or over the limit', () => {
    const onTry = vi.fn();
    panel(onTry);
    fireEvent.click(screen.getByRole('button', { name: 'A buy over the limit' }));
    fireEvent.click(screen.getByRole('button', { name: 'A buy inside the limits' }));
    expect(onTry.mock.calls).toEqual([['over-limit'], ['allowed']]);
  });
});
