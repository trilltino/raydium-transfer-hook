import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { type Connection, Keypair } from '@solana/web3.js';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { key, poolContext, FAIR_LAUNCH_PROGRAM } from './fixtures.ts';

const wallet = vi.hoisted(() => ({
  connected: false,
  publicKey: null as unknown,
  signTransaction: undefined as unknown,
}));
vi.mock('@solana/wallet-adapter-react', () => ({ useWallet: () => wallet }));

const runMock = vi.hoisted(() => vi.fn());
vi.mock('../src/lib/run-swap.ts', () => ({ runHookAwareSwap: runMock }));

import { SwapCard } from '../src/components/SwapCard.tsx';
import { presentSimulationFailure } from '../src/lib/present.ts';

const environment: HookEnvironment = {
  name: 'localnet',
  cluster: 'localnet',
  rpcUrl: 'http://127.0.0.1:8899',
  cpmmProgramId: key(0xc1).toBase58(),
  clmmProgramId: key(0xc2).toBase58(),
  fairLaunchProgramId: FAIR_LAUNCH_PROGRAM.toBase58(),
};

const MINT_A = key(0x11).toBase58();

const WALLET_KEY = Keypair.fromSeed(Buffer.alloc(32, 7)).publicKey;

function connect() {
  wallet.connected = true;
  wallet.publicKey = WALLET_KEY;
  wallet.signTransaction = vi.fn();
}

function renderCard(options: Parameters<typeof poolContext>[0] = {}, balances = { a: 5_000_000_000n, b: 1_000_000_000_000n }) {
  const context = poolContext(options);
  return render(
    <SwapCard
      environment={environment}
      connection={{} as Connection}
      context={context}
      balances={balances}
      reload={async () => context}
      onSwapped={() => undefined}
      loadedAt={Date.now()}
    />
  );
}

async function flipToBuy() {
  await userEvent.click(screen.getByRole('button', { name: 'Switch direction' }));
}

async function typeAmount(text: string) {
  await userEvent.type(screen.getByLabelText('From'), text);
}

beforeEach(() => {
  wallet.connected = false;
  wallet.publicKey = null;
  wallet.signTransaction = undefined;
  runMock.mockReset();
  window.localStorage.setItem('confirmed-mints', JSON.stringify([MINT_A]));
});

describe('wallet states', () => {
  it('asks to connect when the wallet is disconnected', () => {
    renderCard();
    const button = screen.getByRole('button', { name: 'Connect wallet' });
    expect((button as HTMLButtonElement).disabled).toBe(true);
  });

  it('asks for an amount once connected', () => {
    connect();
    renderCard();
    expect((screen.getByRole('button', { name: 'Enter an amount' }) as HTMLButtonElement).disabled).toBe(true);
  });
});

describe('quote and summary', () => {
  it('shows minimum received, price impact and fees for a typed amount', async () => {
    connect();
    renderCard();
    await typeAmount('1000');
    const summary = await screen.findByLabelText('Swap summary');
    expect(within(summary).getByText('Minimum received')).toBeTruthy();
    expect(within(summary).getByText('Price impact')).toBeTruthy();
    expect(within(summary).getByText('Estimated fees')).toBeTruthy();
    expect(within(summary).getByText('Hook')).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Swap' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('refuses more than the balance', async () => {
    connect();
    renderCard({}, { a: 1_000_000n, b: 1_000_000n });
    await typeAmount('5');
    expect(screen.getByRole('button', { name: 'Insufficient balance' })).toBeTruthy();
  });

  it('fills Max and 50% from the balance', async () => {
    connect();
    renderCard({}, { a: 4_000_000n, b: 0n });
    await userEvent.click(screen.getByRole('button', { name: 'Max' }));
    expect((screen.getByLabelText('From') as HTMLInputElement).value).toBe('4');
    await userEvent.click(screen.getByRole('button', { name: '50%' }));
    expect((screen.getByLabelText('From') as HTMLInputElement).value).toBe('2');
  });
});

describe('Fair Launch policy panel', () => {
  it('shows an active window and the protections that are on', () => {
    connect();
    renderCard();
    expect(screen.getByTestId('policy-phase').textContent).toMatch(/^Active/);
    expect(screen.getAllByTestId('policy-meter')).toHaveLength(4);
  });

  it('shows only the limits that are on, and says the fee rule is disabled', () => {
    connect();
    renderCard({ config: { maxBuy: 0n, maxWallet: 0n, maxPriorityMicroLamports: 0n } });
    expect(screen.getAllByTestId('policy-meter')).toHaveLength(1);
    expect(screen.getByText('Priority fee rule disabled')).toBeTruthy();
  });

  it('shows the ended state with no restrictions', () => {
    connect();
    const now = BigInt(Math.floor(Date.now() / 1000));
    renderCard({ config: { windowStart: now - 7_200n, windowEnd: now - 3_600n } });
    expect(screen.getByTestId('policy-phase').textContent).toBe('Fair Launch window ended');
    expect(screen.getByText('Transfers are no longer restricted by this policy.')).toBeTruthy();
    expect(screen.queryAllByTestId('policy-meter')).toHaveLength(0);
  });

  it('shows a not-started window', () => {
    connect();
    const now = BigInt(Math.floor(Date.now() / 1000));
    renderCard({ config: { windowStart: now + 600n, windowEnd: now + 1_200n } });
    expect(screen.getByTestId('policy-phase').textContent).toMatch(/^Not started/);
  });

  it('says buy restrictions do not apply to a sell', () => {
    connect();
    renderCard();
    expect(screen.getByText('Fair Launch buy restrictions do not apply to this sell.')).toBeTruthy();
    expect(screen.queryByText('BUY PROTECTIONS ACTIVE')).toBeNull();
  });

  it('marks a buy as protected and shows it against each limit', async () => {
    connect();
    renderCard();
    await flipToBuy();
    await typeAmount('1000');
    expect(screen.getByText('BUY PROTECTIONS ACTIVE')).toBeTruthy();
    const meters = screen.getAllByTestId('policy-meter');
    expect(meters.every((meter) => meter.getAttribute('data-violated') === 'false')).toBe(true);
    expect(screen.queryByTestId('preflight-warning')).toBeNull();
  });

  it('warns before signing about a max-buy violation but still lets the simulation decide', async () => {
    connect();
    renderCard();
    await flipToBuy();
    await typeAmount('20000');
    expect((await screen.findByTestId('preflight-warning')).textContent).toContain('Buy amount');
    expect((screen.getByRole('button', { name: 'Swap' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('warns about a max-wallet violation', async () => {
    connect();
    renderCard({ config: { maxBuy: 0n } }, { a: 49_500_000_000n, b: 1_000_000_000_000n });
    await flipToBuy();
    await typeAmount('1000');
    expect((await screen.findByTestId('preflight-warning')).textContent).toContain('Wallet after');
  });

  it('warns about a slot-limit violation when the slot is already full', async () => {
    connect();
    renderCard({ counter: { bump: 1, slot: 100n, buys: 3 }, slot: 100n });
    await flipToBuy();
    await typeAmount('10');
    expect((await screen.findByTestId('preflight-warning')).textContent).toContain('Buys this slot');
  });

  it('does not count a full slot from an earlier slot', async () => {
    connect();
    renderCard({ counter: { bump: 1, slot: 99n, buys: 3 }, slot: 100n });
    await flipToBuy();
    await typeAmount('10');
    expect(screen.queryByTestId('preflight-warning')).toBeNull();
  });
});

describe('swap outcomes', () => {
  it('shows a hook refusal as a human-readable reason and says nothing was submitted', async () => {
    connect();
    runMock.mockResolvedValue({
      status: 'blocked',
      failure: presentSimulationFailure(
        { kind: 'hook', programId: FAIR_LAUNCH_PROGRAM.toBase58(), code: 0xb004, codeHex: '0xB004' },
        { fairLaunchProgramId: environment.fairLaunchProgramId }
      ),
    });
    renderCard();
    await flipToBuy();
    await typeAmount('1000');
    await userEvent.click(screen.getByRole('button', { name: 'Swap' }));
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('Swap blocked by Fair Launch');
    expect(screen.getByTestId('failure-reason').textContent).toBe('Your wallet balance would exceed the configured launch limit.');
    expect(alert.textContent).toContain('No transaction was submitted.');
  });

  it('shows the signature after a confirmed swap and refreshes', async () => {
    connect();
    const onSwapped = vi.fn();
    runMock.mockResolvedValue({
      status: 'success',
      signature: 'SigSigSig',
      slot: 7,
      details: { quote: {}, prepared: { kind: 'cpmm', label: 'swap_base_input_v2', input: { slice: [] }, output: { slice: [] } }, simulation: { unitsConsumed: 123456 } },
    });
    const context = poolContext();
    render(
      <SwapCard
        environment={environment}
        connection={{} as Connection}
        context={context}
        balances={{ a: 5_000_000_000n, b: 1_000_000_000_000n }}
        reload={async () => context}
        onSwapped={onSwapped}
        loadedAt={Date.now()}
      />
    );
    await typeAmount('1');
    await userEvent.click(screen.getByRole('button', { name: 'Swap' }));
    expect(await screen.findByText('Swap confirmed')).toBeTruthy();
    expect(screen.getAllByText('SigSigSig').length).toBeGreaterThan(0);
    await waitFor(() => expect(onSwapped).toHaveBeenCalled());
  });

  it('puts the instruction and compute in the developer details', async () => {
    connect();
    runMock.mockResolvedValue({
      status: 'success',
      signature: 'SigSigSig',
      slot: 7,
      details: { quote: {}, prepared: { kind: 'cpmm', label: 'swap_base_input_v2', input: { slice: [{ pubkey: key(5) }] }, output: { slice: [] } }, simulation: { unitsConsumed: 123456 } },
    });
    renderCard();
    await typeAmount('1');
    await userEvent.click(screen.getByRole('button', { name: 'Swap' }));
    await screen.findByText('Swap confirmed');
    const details = screen.getByTestId('developer-details');
    expect(details.textContent).toContain('CPMM  swap_base_input_v2');
    expect(details.textContent).toContain('123456 units');
    expect(details.textContent).toContain('Input hook accounts (1)');
    expect(details.textContent).toContain('Output hook accounts (0)');
    expect(details.hasAttribute('open')).toBe(false);
  });
});

describe('unknown token confirmation', () => {
  it('blocks the swap until the mint is confirmed, and remembers it', async () => {
    connect();
    window.localStorage.clear();
    renderCard();
    expect(screen.getByTestId('unknown-token').textContent).toContain(MINT_A);
    await typeAmount('1');
    expect((screen.getByRole('button', { name: 'Confirm the token first' }) as HTMLButtonElement).disabled).toBe(true);
    await userEvent.click(screen.getByRole('button', { name: 'I understand, continue' }));
    expect(screen.queryByTestId('unknown-token')).toBeNull();
    expect((screen.getByRole('button', { name: 'Swap' }) as HTMLButtonElement).disabled).toBe(false);
    expect(window.localStorage.getItem('confirmed-mints')).toContain(MINT_A);
  });
});

describe('layout rules', () => {
  it('keeps inputs at 16px and touch targets at 44px for small screens', async () => {
    const { readFileSync } = await import('node:fs');
    const css = readFileSync(`${process.cwd()}/src/styles.css`, 'utf8');
    expect(css).toContain('@media (max-width: 719px)');
    expect(css).toContain('@media (min-width: 1100px)');
    expect(css).toMatch(/\.btn\s*\{[^}]*min-height:\s*44px/);
    expect(css).toMatch(/input\.amount\s*\{[^}]*font-size:\s*max\(16px/);
    expect(css).toContain('prefers-reduced-motion');
  });
});

void fireEvent;
