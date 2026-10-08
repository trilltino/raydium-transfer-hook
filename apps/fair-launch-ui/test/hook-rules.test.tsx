import type { CreatorCommitmentConfig, FairLaunchConfig, HookEnvironment } from '@raydium-transfer-hook/client';
import { PublicKey } from '@solana/web3.js';
import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { TransactionTrace } from '../src/components/TransactionTrace.tsx';
import { type HookPolicy, enforcedRules } from '../src/lib/hook-rules.ts';
import { traceSourceFor } from '../src/lib/trace-client.ts';
import { type TraceStep, type TraceView, buildTrace } from '../src/lib/trace.ts';
import buy from './data/fair-launch-buy.json';

// A real Fair Launch buy on our devnet pool, read back through Triton One: 5 tokens out of the pool, with the
// pool's launch settings (max buy 100, wallet cap 300, 3 buys per slot, priority fee 1000) as it was created with.
const devnet: HookEnvironment = {
  name: 'integration-devnet',
  cluster: 'devnet',
  rpcUrl: 'https://api.devnet.solana.com',
  cpmmProgramId: '7tRJH4mmEfNGGLf9E8qEvo3oSjq2b75DhSggb1Wz45fJ',
  clmmProgramId: '3dNJapViueBArDN3HbWKcEf2u6hfZQ3oDyh3GmUXJ8oD',
  fairLaunchProgramId: '7xyk1AQg7xCaQucPgWs13hmAs4dmD214raNgEZhLPQSu',
};
const rpcFetch = ((_url: unknown, init?: RequestInit) =>
  Promise.resolve(new Response(JSON.stringify({ jsonrpc: '2.0', id: JSON.parse(String(init?.body)).id, result: buy })))) as unknown as typeof fetch;
const source = () => traceSourceFor(devnet, { triton: true, origin: 'http://x', fetch: rpcFetch });

async function loaded(): Promise<{ trace: TraceView; hook: TraceStep }> {
  const trace = buildTrace((await source().fetchTransaction('sig'))!, devnet, 'Triton One (devnet)');
  const hook = trace.steps.find((step) => step.isHook)!;
  return { trace, hook };
}

const config = (venue: string, overrides: Partial<FairLaunchConfig> = {}): FairLaunchConfig => ({
  bump: 255,
  mint: new PublicKey('5kFiy1TyxEufoL7jmBvDNMMbK7NcxaifJSkAxayNvEkd'),
  venues: [new PublicKey(venue)],
  windowStart: 0n,
  windowEnd: 99_999_999_999n,
  maxBuy: 100_000_000n,
  maxWallet: 300_000_000n,
  maxBuysPerSlot: 3,
  maxPriorityMicroLamports: 1000n,
  ...overrides,
});
const policy = (venue: string, overrides?: Partial<FairLaunchConfig>): HookPolicy => ({ kind: 'fair-launch', config: config(venue, overrides), decimals: 6 });

describe('naming what the Fair Launch hook enforced', () => {
  it('lists each rule of the launch with this buy’s numbers against the limits, all passed', async () => {
    const { trace, hook } = await loaded();
    const enforcement = enforcedRules(policy(hook.accounts[0]), trace, hook)!;
    expect(enforcement.headline).toContain('max buy · wallet cap · buys per slot · priority fee · launch window');
    expect(enforcement.lines.map((line) => [line.name, line.status])).toEqual([
      ['Max buy per transaction', 'passed'],
      ['Wallet cap after the buy', 'passed'],
      ['Buys per slot', 'passed'],
      ['Priority fee', 'passed'],
      ['Launch window', 'passed'],
    ]);
    expect(enforcement.lines[0].detail).toBe('5 of at most 100');
    expect(enforcement.lines[1].detail).toMatch(/of at most 300 held$/);
    expect(enforcement.lines[3].detail).toBe('0 µ-lamports declared, at most 1000 allowed');
  });

  it('leaves out a rule the launch switched off', async () => {
    const { trace, hook } = await loaded();
    const enforcement = enforcedRules(policy(hook.accounts[0], { maxWallet: 0n, maxPriorityMicroLamports: 0n }), trace, hook)!;
    expect(enforcement.lines.map((line) => line.name)).toEqual(['Max buy per transaction', 'Buys per slot', 'Launch window']);
  });

  it('says the rules did not apply when the transfer is not out of a launch pool', async () => {
    const { trace, hook } = await loaded();
    const enforcement = enforcedRules(policy(PublicKey.unique().toBase58()), trace, hook)!;
    expect(enforcement.headline).toContain('not a buy');
    expect(enforcement.lines).toEqual([expect.objectContaining({ status: 'skipped' })]);
  });

  it('marks the rule the hook refused over, from the error it returned', async () => {
    const { trace, hook } = await loaded();
    const refused: TraceStep = { ...hook, failed: true, logs: ['Program 7xyk failed: custom program error: 0xb003'] };
    const enforcement = enforcedRules(policy(hook.accounts[0]), trace, refused)!;
    expect(enforcement.lines.find((line) => line.name === 'Max buy per transaction')?.status).toBe('failed');
    expect(enforcement.lines.find((line) => line.name === 'Wallet cap after the buy')?.status).toBe('passed');
    expect(enforcement.lines.at(-1)).toMatchObject({ name: 'Refused', status: 'failed', detail: 'This buy is larger than the launch max-buy limit.' });
  });

  it('says what Creator Commitment and Holder Rewards do on the same transfer', async () => {
    const { trace, hook } = await loaded();
    const creator: CreatorCommitmentConfig = {
      bump: 255,
      mint: new PublicKey('5kFiy1TyxEufoL7jmBvDNMMbK7NcxaifJSkAxayNvEkd'),
      creatorAccount: PublicKey.unique(),
      lockedTotal: 80_000_000n,
      start: 0n,
      cliff: 10n,
      end: 99_999_999_999n,
    };
    const floor = enforcedRules({ kind: 'creator-commitment', config: creator, decimals: 6 }, trace, hook)!;
    expect(floor.lines[0]).toMatchObject({ name: 'Vesting floor', status: 'skipped' });
    const rewards = enforcedRules({ kind: 'holder-rewards' }, trace, hook)!;
    expect(rewards.lines[0]).toMatchObject({ name: 'Reward accounting', status: 'info' });
    expect(enforcedRules(undefined, trace, hook)).toBeNull();
  });
});

describe('the trace card names the enforced rules under the hook step', () => {
  it('shows them with the hook’s Execute, and only there', async () => {
    const { hook } = await loaded();
    render(<TransactionTrace environment={devnet} signature="sig" source={source()} policy={policy(hook.accounts[0])} />);
    await screen.findByTestId('trace-summary');
    const steps = screen.getAllByTestId('trace-step');
    const withRules = steps.filter((step) => step.querySelector('[data-testid="trace-rules"]'));
    expect(withRules).toHaveLength(1);
    const rules = within(withRules[0]).getByTestId('trace-rules');
    expect(rules.textContent).toContain('Max buy per transaction');
    expect(rules.textContent).toContain('5 of at most 100');
    expect([...rules.querySelectorAll('li')].every((li) => li.getAttribute('data-status') === 'passed')).toBe(true);
    expect(within(withRules[0]).getByText('Execute (Transfer Hook)')).toBeTruthy();
  });
});
