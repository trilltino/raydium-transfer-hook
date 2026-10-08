import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { key, poolContext } from './fixtures.ts';

const wallet = vi.hoisted(() => ({ connected: false, publicKey: null as unknown, signTransaction: undefined as unknown, wallets: [], wallet: null }));
vi.mock('@solana/wallet-adapter-react', () => ({ useWallet: () => wallet }));

const poolState = vi.hoisted(() => ({ current: { status: 'none' } as unknown }));
vi.mock('../src/hooks/usePool.ts', () => ({
  usePool: () => ({ state: poolState.current, refresh: async () => undefined, load: async () => undefined }),
}));
vi.mock('../src/hooks/useRaydium.ts', () => ({
  useRaydium: () => ({ connection: {}, raydium: {}, error: null }),
}));
vi.mock('../src/hooks/useWalletBalances.ts', () => ({
  useWalletBalances: () => ({ balances: null, refresh: async () => undefined }),
}));

import { App } from '../src/App.tsx';

function open(search: string) {
  window.history.replaceState(null, '', `/${search}`);
  return render(<App />);
}

beforeEach(() => {
  poolState.current = { status: 'none' };
  window.localStorage.clear();
});

describe('App', () => {
  it('is a devnet page: a network label and the Raydium mark, no environment switch and no banner', () => {
    open('');
    expect(screen.getByText('Devnet')).toBeTruthy();
    expect(screen.getByRole('img', { name: 'Raydium' })).toBeTruthy();
    expect(screen.queryByLabelText('Environment')).toBeNull();
    expect(screen.queryByText(/Experimental Raydium Transfer Hook environment/)).toBeNull();
  });

  it('offers a pool picker when no pool is given', () => {
    open('');
    expect(screen.getByText('Open a pool')).toBeTruthy();
  });

  it('rejects a malformed ?pool= value', () => {
    open('?pool=nonsense');
    expect(screen.getByRole('alert').textContent).toContain('is not a valid pool address');
  });

  it('shows a loading state while the pool loads', () => {
    poolState.current = { status: 'loading' };
    open(`?pool=${key(1).toBase58()}`);
    expect(screen.getByTestId('loading-pool')).toBeTruthy();
  });

  it('fails closed with the reason when the pool does not match', () => {
    poolState.current = { status: 'blocked', context: poolContext(), reason: 'This pool belongs to another program.' };
    open(`?pool=${key(1).toBase58()}`);
    expect(screen.getByTestId('pool-blocked').textContent).toBe('This pool belongs to another program.');
    expect(screen.queryByRole('button', { name: 'Swap' })).toBeNull();
  });

  it('shows a load error', () => {
    poolState.current = { status: 'error', message: 'fetch pool info error' };
    open(`?pool=${key(1).toBase58()}`);
    expect(screen.getByRole('alert').textContent).toContain('fetch pool info error');
  });
});
