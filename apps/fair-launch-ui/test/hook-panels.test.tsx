import type { CreatorCommitmentConfig, HolderRewardsGlobal, HolderRecord, HookEnvironment } from '@raydium-transfer-hook/client';
import { getAssociatedTokenAddressSync } from '@solana/spl-token';
import { type Connection, Keypair } from '@solana/web3.js';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { CREATOR_PROGRAM, FAIR_LAUNCH_PROGRAM, REWARDS_PROGRAM, key, poolContext } from './fixtures.ts';

const WALLET_KEY = Keypair.fromSeed(Buffer.alloc(32, 7)).publicKey;

const wallet = vi.hoisted(() => ({
  connected: false,
  publicKey: null as unknown,
  signTransaction: undefined as unknown,
}));
vi.mock('@solana/wallet-adapter-react', () => ({ useWallet: () => wallet }));

const runSwap = vi.hoisted(() => vi.fn());
vi.mock('../src/lib/run-swap.ts', () => ({ runHookAwareSwap: runSwap }));

const runTx = vi.hoisted(() => vi.fn());
vi.mock('../src/lib/run-transaction.ts', () => ({ runTransaction: runTx }));

const rewardsAccount = vi.hoisted(() => ({ current: null as unknown, refresh: vi.fn(async () => undefined) }));
vi.mock('../src/hooks/useRewardsAccount.ts', () => ({
  useRewardsAccount: () => ({ account: rewardsAccount.current, error: null, refresh: rewardsAccount.refresh }),
}));

import { CreatorCommitmentPolicy } from '../src/components/CreatorCommitmentPolicy.tsx';
import { HolderRewardsPanel } from '../src/components/HolderRewardsPanel.tsx';
import { SwapCard } from '../src/components/SwapCard.tsx';
import { presentContext, presentSimulationFailure } from '../src/lib/present.ts';

const environment: HookEnvironment = {
  name: 'localnet',
  cluster: 'localnet',
  rpcUrl: 'http://127.0.0.1:8899',
  cpmmProgramId: key(0xc1).toBase58(),
  clmmProgramId: key(0xc2).toBase58(),
  fairLaunchProgramId: FAIR_LAUNCH_PROGRAM.toBase58(),
  creatorCommitmentProgramId: CREATOR_PROGRAM.toBase58(),
  holderRewardsProgramId: REWARDS_PROGRAM.toBase58(),
};

const nowSeconds = () => BigInt(Math.floor(Date.now() / 1000));
const MINT_A = key(0x11);

function connect() {
  wallet.connected = true;
  wallet.publicKey = WALLET_KEY;
  wallet.signTransaction = vi.fn();
}

beforeEach(() => {
  wallet.connected = false;
  wallet.publicKey = null;
  wallet.signTransaction = undefined;
  runSwap.mockReset();
  runTx.mockReset();
  rewardsAccount.current = null;
  rewardsAccount.refresh.mockClear();
  window.localStorage.setItem('confirmed-mints', JSON.stringify([MINT_A.toBase58()]));
});

function commitment(overrides: Partial<CreatorCommitmentConfig> = {}): CreatorCommitmentConfig {
  const walletAccount = getAssociatedTokenAddressSync(MINT_A, WALLET_KEY, false, key(0x21));
  const now = nowSeconds();
  return {
    bump: 255,
    mint: MINT_A,
    creatorAccount: walletAccount,
    lockedTotal: 80_000_000n,
    start: now - 100n,
    cliff: now - 50n,
    end: now + 3_600n,
    ...overrides,
  };
}

function renderCard(context: ReturnType<typeof poolContext>, balances = { a: 100_000_000n, b: 1_000_000_000n }) {
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

describe('Creator Commitment panel', () => {
  it('shows the schedule: locked in total, locked now, and the creator account', () => {
    connect();
    renderCard(poolContext({ commitment: commitment() }));
    expect(screen.getByText('Creator commitment')).toBeTruthy();
    expect(screen.getByTestId('locked-total').textContent).toBe('80');
    // vesting for 100 of 3,700 seconds: a little under 80 is still locked
    const lockedNow = Number(screen.getByTestId('locked-now').textContent);
    expect(lockedNow).toBeGreaterThan(77);
    expect(lockedNow).toBeLessThan(80);
    expect(screen.getByTestId('creator-account').textContent).toBe(commitment().creatorAccount.toBase58());
    expect(screen.getByTestId('policy-phase').textContent).toMatch(/^Vesting/);
    expect(screen.getByText('YOUR WALLET IS THE CREATOR ACCOUNT')).toBeTruthy();
  });

  it('warns before signing about a sale that would breach the floor, and still lets the simulation decide', async () => {
    connect();
    renderCard(poolContext({ commitment: commitment() }));
    await userEvent.type(screen.getByLabelText('From'), '30'); // 100 held, ~78 locked: 70 would be left
    expect((await screen.findByTestId('preflight-warning')).textContent).toContain('vesting floor');
    expect(screen.getByTestId('floor-check').getAttribute('data-violated')).toBe('true');
    expect((screen.getByRole('button', { name: 'Swap' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('shows no warning for a sale that leaves the floor intact', async () => {
    connect();
    renderCard(poolContext({ commitment: commitment() }));
    await userEvent.type(screen.getByLabelText('From'), '10');
    await screen.findByTestId('floor-check');
    expect(screen.getByTestId('floor-check').getAttribute('data-violated')).toBe('false');
    expect(screen.queryByTestId('preflight-warning')).toBeNull();
  });

  it('says nothing about the floor when the wallet is not the creator account', async () => {
    connect();
    renderCard(poolContext({ commitment: commitment({ creatorAccount: key(0x66) }) }));
    await userEvent.type(screen.getByLabelText('From'), '30');
    expect(screen.queryByTestId('floor-check')).toBeNull();
    expect(screen.queryByText('YOUR WALLET IS THE CREATOR ACCOUNT')).toBeNull();
  });

  it('shows a fully vested schedule and the not-yet-started cliff', () => {
    const now = nowSeconds();
    const done = render(
      <CreatorCommitmentPolicy
        config={commitment({ start: now - 500n, cliff: now - 400n, end: now - 10n })}
        now={now}
        decimals={6}
        hookProgramId="Hook"
        tokenLabel="T"
        floorCheck={null}
        walletIsCreator={false}
      />
    );
    expect(done.getByTestId('policy-phase').textContent).toBe('Fully vested');
    expect(done.getByTestId('locked-now').textContent).toBe('0');
    done.unmount();
    const before = render(
      <CreatorCommitmentPolicy
        config={commitment({ start: now - 10n, cliff: now + 500n, end: now + 900n })}
        now={now}
        decimals={6}
        hookProgramId="Hook"
        tokenLabel="T"
        floorCheck={null}
        walletIsCreator={false}
      />
    );
    expect(before.getByTestId('policy-phase').textContent).toMatch(/^Locked until the cliff/);
    expect(before.getByTestId('locked-now').textContent).toBe('80');
  });

  it('explains a vesting-floor refusal in words', () => {
    const view = presentSimulationFailure(
      { kind: 'hook', programId: CREATOR_PROGRAM.toBase58(), code: 0xa005, codeHex: '0xA005' },
      presentContext(environment)
    );
    expect(view).toMatchObject({ source: 'creator-commitment', rule: 'VestingFloorBreached', notSubmitted: true });
    expect(view.reason).toContain('below the amount still locked');
  });
});

describe('Holder Rewards panel', () => {
  const rewards = (overrides: Partial<HolderRewardsGlobal['stream']> = {}): HolderRewardsGlobal => ({
    bump: 255,
    mint: MINT_A,
    rewardMint: key(0x41),
    rewardVault: key(0x42),
    poolVault: key(0x31),
    oneTime: false,
    stream: {
      rate: 1_000_000n, // one reward token per second
      periodFinish: nowSeconds() + 1_000n,
      lastUpdate: nowSeconds() - 10n,
      index: 0n,
      eligibleSupply: 100_000_000n,
      ...overrides,
    },
  });

  const record = (overrides: Partial<HolderRecord> = {}): HolderRecord => ({
    bump: 255,
    tokenAccount: key(0x55),
    checkpoint: 100_000_000n,
    indexPaid: 0n,
    earned: 0n,
    ...overrides,
  });

  function renderPanel(global: HolderRewardsGlobal, onChanged = vi.fn()) {
    const context = poolContext({ rewards: global });
    const onSwapped = onChanged;
    render(
      <HolderRewardsPanel
        environment={environment}
        connection={{} as Connection}
        context={context}
        rewards={context.rewards!}
        tokenLabel="T"
        onChanged={onSwapped}
      />
    );
    return onChanged;
  }

  const account = (overrides: Record<string, unknown> = {}) => ({
    tokenAccount: key(0x55),
    record: null,
    balance: 100_000_000n,
    rewardBalance: 0n,
    ...overrides,
  });

  it('asks to connect a wallet', () => {
    renderPanel(rewards());
    expect(screen.getByText('Connect a wallet to see your rewards.')).toBeTruthy();
    expect(screen.getByTestId('policy-phase').textContent).toMatch(/^Paying 1/);
  });

  it('offers Register to an unregistered holder and keeps Claim off', () => {
    connect();
    rewardsAccount.current = account();
    renderPanel(rewards());
    expect(screen.getByTestId('registered').textContent).toBe('No');
    expect((screen.getByRole('button', { name: 'Register' }) as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByRole('button', { name: 'Claim' }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/Only registered accounts earn/)).toBeTruthy();
  });

  it('will not offer Register to an account that holds none of the token', () => {
    connect();
    rewardsAccount.current = account({ balance: 0n });
    renderPanel(rewards());
    expect((screen.getByRole('button', { name: 'Register' }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/Hold some of this token first/)).toBeTruthy();
  });

  it('shows what a registered holder has earned and lets it claim', () => {
    connect();
    rewardsAccount.current = account({ record: record({ earned: 2_000_000n }) });
    renderPanel(rewards());
    expect(screen.getByTestId('registered').textContent).toBe('Yes');
    // settled 2 plus about 10 seconds of the stream since it was last updated
    const claimable = Number(screen.getByTestId('claimable').textContent);
    expect(claimable).toBeGreaterThanOrEqual(11);
    expect(claimable).toBeLessThan(40);
    expect((screen.getByRole('button', { name: 'Register' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'Claim' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('registers: simulates through the shared runner, then refreshes', async () => {
    connect();
    rewardsAccount.current = account();
    runTx.mockResolvedValue({ status: 'success', signature: 'RegSig', slot: 5, simulation: {} });
    const onChanged = renderPanel(rewards());
    await userEvent.click(screen.getByRole('button', { name: 'Register' }));
    expect((await screen.findByTestId('action-status')).textContent).toContain('Registered');
    const request = runTx.mock.calls[0][0];
    expect(request.instructions).toHaveLength(1);
    expect(request.instructions[0].programId.toBase58()).toBe(REWARDS_PROGRAM.toBase58());
    expect([...request.instructions[0].data]).toEqual([1]);
    expect(request.hookPrograms.map((k: { toBase58(): string }) => k.toBase58())).toEqual([REWARDS_PROGRAM.toBase58()]);
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(rewardsAccount.refresh).toHaveBeenCalled();
  });

  it('claims through a created reward account, and shows a refusal in words', async () => {
    connect();
    rewardsAccount.current = account({ record: record({ earned: 5_000_000n }) });
    runTx.mockResolvedValue({
      status: 'blocked',
      failure: presentSimulationFailure(
        { kind: 'hook', programId: REWARDS_PROGRAM.toBase58(), code: 0xc00c, codeHex: '0xC00C' },
        presentContext(environment, 'action')
      ),
    });
    const onChanged = renderPanel(rewards());
    await userEvent.click(screen.getByRole('button', { name: 'Claim' }));
    const status = await screen.findByTestId('action-status');
    expect(status.textContent).toContain('Rewards action blocked');
    expect(screen.getByTestId('failure-reason').textContent).toBe('There is nothing to claim yet.');
    expect(status.textContent).toContain('No transaction was submitted.');
    // an idempotent create of the reward account, then the claim
    const request = runTx.mock.calls[0][0];
    expect(request.instructions).toHaveLength(2);
    expect([...request.instructions[1].data]).toEqual([3]);
    expect(onChanged).not.toHaveBeenCalled();
  });

  it('says plainly when the stream is not funded or has ended', () => {
    connect();
    rewardsAccount.current = account();
    const { unmount } = render(<div />);
    unmount();
    renderPanel(rewards({ rate: 0n, periodFinish: 0n }));
    expect(screen.getByTestId('policy-phase').textContent).toBe('Not funded yet');
  });
});
