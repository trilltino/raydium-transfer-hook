import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { type Connection, Keypair, LAMPORTS_PER_SOL } from '@solana/web3.js';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { GetSol } from '../src/components/GetSol.tsx';

const base: HookEnvironment = {
  name: 'localnet',
  cluster: 'localnet',
  rpcUrl: 'http://127.0.0.1:8899',
  cpmmProgramId: '7tRJH4mmEfNGGLf9E8qEvo3oSjq2b75DhSggb1Wz45fJ',
  clmmProgramId: '3dNJapViueBArDN3HbWKcEf2u6hfZQ3oDyh3GmUXJ8oD',
  fairLaunchProgramId: '7xyk1AQg7xCaQucPgWs13hmAs4dmD214raNgEZhLPQSu',
};
const wallet = Keypair.fromSeed(Buffer.alloc(32, 4)).publicKey;

function connection(balances: number[], airdrop: () => Promise<string> = async () => 'SIG') {
  const queue = [...balances];
  return {
    getBalance: vi.fn(async () => (queue.length > 1 ? queue.shift()! : queue[0])),
    requestAirdrop: vi.fn(airdrop),
    getLatestBlockhash: vi.fn(async () => ({ blockhash: 'h', lastValidBlockHeight: 1 })),
    confirmTransaction: vi.fn(async () => ({ value: { err: null } })),
  } as unknown as Connection & { requestAirdrop: ReturnType<typeof vi.fn> };
}

describe('Get SOL', () => {
  it('is not shown when the wallet already has enough SOL', async () => {
    const c = connection([LAMPORTS_PER_SOL]);
    render(<GetSol environment={base} connection={c} wallet={wallet} />);
    await waitFor(() => expect(c.getBalance).toHaveBeenCalled());
    expect(screen.queryByTestId('get-sol')).toBeNull();
  });

  it('is shown for an empty wallet, asks the faucet for 1 SOL, and disappears once the SOL arrives', async () => {
    const c = connection([0, LAMPORTS_PER_SOL]);
    const funded = vi.fn();
    render(<GetSol environment={base} connection={c} wallet={wallet} onFunded={funded} />);
    expect((await screen.findByTestId('get-sol')).textContent).toContain('0.000 SOL on the local validator');
    fireEvent.click(screen.getByRole('button', { name: 'Get 1 SOL' }));
    await waitFor(() => expect(screen.queryByTestId('get-sol')).toBeNull());
    expect(c.requestAirdrop).toHaveBeenCalledWith(wallet, LAMPORTS_PER_SOL);
    expect(funded).toHaveBeenCalledOnce();
  });

  it('on devnet says the faucet is rate-limited and points at the web faucet when it refuses', async () => {
    const c = connection([0], async () => {
      throw new Error('429 Too Many Requests');
    });
    render(<GetSol environment={{ ...base, name: 'integration-devnet', cluster: 'devnet' }} connection={c} wallet={wallet} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Get 1 SOL' }));
    const error = await screen.findByTestId('get-sol-error');
    expect(error.textContent).toContain('rate-limited');
    expect(error.textContent).toContain('faucet.solana.com');
  });
});
