import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PublicKey } from '@solana/web3.js';
import { describe, expect, it } from 'vitest';
import {
  CONFIG_LEN,
  COUNTER_LEN,
  FAIR_LAUNCH_ERRORS,
  type FairLaunchConfig,
  decodeFairLaunchConfig,
  decodeFairLaunchCounter,
  getFairLaunchConfigAddress,
  getFairLaunchCounterAddress,
  launchPhase,
  previewBuy,
  readFairLaunchConfig,
} from '../src/index.ts';
import { MemoryConnection, key } from './chain.ts';

/** Written by `templates/fair-launch` (`UPDATE_GOLDEN=1 cargo test -p fair-launch-hook typescript_fixture`). */
const fixture = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'tests', 'fixtures', 'typescript', 'fair-launch.json'), 'utf8')
) as {
  program_id: string;
  mint: string;
  venues: string[];
  config_address: string;
  counter_address: string;
  config_hex: string;
  counter_hex: string;
  params: Record<string, number>;
  counter: { slot: number; buys: number };
  errors: { name: string; code: number }[];
};

describe('the Rust fair-launch fixture', () => {
  const program = new PublicKey(fixture.program_id);
  const mint = new PublicKey(fixture.mint);

  it('derives the same config and counter addresses', () => {
    expect(getFairLaunchConfigAddress(mint, program).toBase58()).toBe(fixture.config_address);
    expect(getFairLaunchCounterAddress(mint, program).toBase58()).toBe(fixture.counter_address);
  });

  it('decodes the 214-byte config', () => {
    const bytes = Buffer.from(fixture.config_hex, 'hex');
    expect(bytes.length).toBe(CONFIG_LEN);
    expect(CONFIG_LEN).toBe(214);
    const config = decodeFairLaunchConfig(bytes);
    expect(config.mint.toBase58()).toBe(fixture.mint);
    expect(config.venues.map((venue) => venue.toBase58())).toEqual(fixture.venues);
    expect(config.windowStart).toBe(BigInt(fixture.params.window_start));
    expect(config.windowEnd).toBe(BigInt(fixture.params.window_end));
    expect(config.maxBuy).toBe(BigInt(fixture.params.max_buy));
    expect(config.maxWallet).toBe(BigInt(fixture.params.max_wallet));
    expect(config.maxBuysPerSlot).toBe(fixture.params.max_buys_per_slot);
    expect(config.maxPriorityMicroLamports).toBe(BigInt(fixture.params.max_priority_micro_lamports));
  });

  it('decodes the 21-byte counter', () => {
    const bytes = Buffer.from(fixture.counter_hex, 'hex');
    expect(bytes.length).toBe(COUNTER_LEN);
    expect(decodeFairLaunchCounter(bytes)).toMatchObject({ slot: BigInt(fixture.counter.slot), buys: fixture.counter.buys });
  });

  it('knows every error code the template defines', () => {
    expect(FAIR_LAUNCH_ERRORS.map(({ name, code }) => ({ name, code })).sort((a, b) => a.code - b.code)).toEqual(
      [...fixture.errors].sort((a, b) => a.code - b.code)
    );
  });

  it('reads the config from an account the program owns, and ignores one it does not', async () => {
    const address = new PublicKey(fixture.config_address);
    const owned = new MemoryConnection().set(address, program, Buffer.from(fixture.config_hex, 'hex'));
    expect((await readFairLaunchConfig(owned.asConnection(), mint, program))?.venues).toHaveLength(2);
    const foreign = new MemoryConnection().set(address, key(0x99), Buffer.from(fixture.config_hex, 'hex'));
    expect(await readFairLaunchConfig(foreign.asConnection(), mint, program)).toBeNull();
    expect(await readFairLaunchConfig(new MemoryConnection().asConnection(), mint, program)).toBeNull();
  });

  it('refuses malformed accounts', () => {
    const bytes = Buffer.from(fixture.config_hex, 'hex');
    expect(() => decodeFairLaunchConfig(bytes.subarray(0, CONFIG_LEN - 1))).toThrow();
    const noVenues = Buffer.from(bytes);
    noVenues[41] = 0;
    expect(() => decodeFairLaunchConfig(noVenues)).toThrow();
    const wrongTag = Buffer.from(bytes);
    wrongTag[0] ^= 1;
    expect(() => decodeFairLaunchConfig(wrongTag)).toThrow();
  });
});

describe('policy preview', () => {
  const base: FairLaunchConfig = {
    bump: 255,
    mint: key(1),
    venues: [key(2)],
    windowStart: 100n,
    windowEnd: 200n,
    maxBuy: 10_000n,
    maxWallet: 50_000n,
    maxBuysPerSlot: 3,
    maxPriorityMicroLamports: 1_000n,
  };
  const buy = { amount: 8_000n, walletBalance: 31_500n, currentSlot: 9n, priorityMicroLamports: 500n };

  it('tracks the window as half-open', () => {
    expect(launchPhase(base, 99n)).toBe('not-started');
    expect(launchPhase(base, 100n)).toBe('active');
    expect(launchPhase(base, 199n)).toBe('active');
    expect(launchPhase(base, 200n)).toBe('ended');
  });

  it('shows only the limits that are on', () => {
    const rows = previewBuy({ ...base, maxBuy: 0n, maxWallet: 0n, maxPriorityMicroLamports: 0n }, null, buy, 150n);
    expect(rows.map((row) => row.rule)).toEqual(['buys-per-slot']);
  });

  it('uses the post-buy balance and counts this buy in the slot', () => {
    const rows = previewBuy(base, { bump: 1, slot: 9n, buys: 2 }, buy, 150n);
    expect(rows.find((row) => row.rule === 'max-wallet')).toMatchObject({ used: 39_500n, violated: false });
    expect(rows.find((row) => row.rule === 'buys-per-slot')).toMatchObject({ used: 3n, violated: false });
    const full = previewBuy(base, { bump: 1, slot: 9n, buys: 3 }, buy, 150n);
    expect(full.find((row) => row.rule === 'buys-per-slot')?.violated).toBe(true);
    const newSlot = previewBuy(base, { bump: 1, slot: 8n, buys: 3 }, buy, 150n);
    expect(newSlot.find((row) => row.rule === 'buys-per-slot')?.used).toBe(1n);
  });

  it('flags a buy exactly one over each limit and passes one exactly at it', () => {
    const at = previewBuy(base, null, { ...buy, amount: 10_000n, walletBalance: 40_000n, priorityMicroLamports: 1_000n }, 150n);
    expect(at.every((row) => !row.violated)).toBe(true);
    const over = previewBuy(base, null, { ...buy, amount: 10_001n, walletBalance: 40_000n, priorityMicroLamports: 1_001n }, 150n);
    expect(over.filter((row) => row.violated).map((row) => row.rule)).toEqual(['max-buy', 'max-wallet', 'priority-fee']);
  });

  it('shows nothing outside the window', () => {
    expect(previewBuy(base, null, buy, 99n)).toEqual([]);
    expect(previewBuy(base, null, buy, 200n)).toEqual([]);
  });
});
