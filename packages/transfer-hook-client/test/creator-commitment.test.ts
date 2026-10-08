import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PublicKey } from '@solana/web3.js';
import { describe, expect, it } from 'vitest';
import {
  COMMITMENT_CONFIG_LEN,
  COMMITMENT_ERRORS,
  decodeCreatorCommitmentConfig,
  decodeCreatorCommitmentError,
  getCreatorCommitmentConfigAddress,
  lockedAt,
  previewCreatorTransfer,
  readCreatorCommitmentConfig,
  vestingView,
} from '../src/index.ts';
import { MemoryConnection } from './chain.ts';

/** Written by `templates/creator-commitment` (`UPDATE_GOLDEN=1 cargo test -p creator-commitment-hook typescript_fixture`). */
const fixture = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'tests', 'fixtures', 'typescript', 'creator-commitment.json'), 'utf8')
) as {
  program_id: string;
  mint: string;
  creator_account: string;
  config_address: string;
  config_hex: string;
  schedule: { locked_total: number; start: number; cliff: number; end: number };
  locked_at: { now: number; locked: number }[];
  errors: { name: string; code: number }[];
};

const program = new PublicKey(fixture.program_id);
const mint = new PublicKey(fixture.mint);
const bytes = Buffer.from(fixture.config_hex, 'hex');

describe('the Rust creator-commitment fixture', () => {
  it('derives the same config address', () => {
    expect(getCreatorCommitmentConfigAddress(mint, program).toBase58()).toBe(fixture.config_address);
  });

  it('decodes the 105-byte config', () => {
    expect(bytes.length).toBe(COMMITMENT_CONFIG_LEN);
    const config = decodeCreatorCommitmentConfig(bytes);
    expect(config.mint.toBase58()).toBe(fixture.mint);
    expect(config.creatorAccount.toBase58()).toBe(fixture.creator_account);
    expect(config.lockedTotal).toBe(BigInt(fixture.schedule.locked_total));
    expect(config.start).toBe(BigInt(fixture.schedule.start));
    expect(config.cliff).toBe(BigInt(fixture.schedule.cliff));
    expect(config.end).toBe(BigInt(fixture.schedule.end));
  });

  it('computes the locked amount exactly as the program does, at every sampled time', () => {
    const config = decodeCreatorCommitmentConfig(bytes);
    for (const sample of fixture.locked_at) {
      expect(lockedAt(config, BigInt(sample.now)), `at ${sample.now}`).toBe(BigInt(sample.locked));
    }
  });

  it('knows every error code the template defines', () => {
    expect(COMMITMENT_ERRORS.map(({ name, code }) => ({ name, code })).sort((a, b) => a.code - b.code)).toEqual(
      [...fixture.errors].sort((a, b) => a.code - b.code)
    );
    expect(decodeCreatorCommitmentError(0xa005)?.tradeRule).toBe(true);
    expect(decodeCreatorCommitmentError(0xb003)).toBeNull();
  });

  it('refuses malformed accounts', () => {
    expect(() => decodeCreatorCommitmentConfig(bytes.subarray(0, bytes.length - 1))).toThrow();
    const wrongTag = Buffer.from(bytes);
    wrongTag[0] ^= 1;
    expect(() => decodeCreatorCommitmentConfig(wrongTag)).toThrow();
  });

  it('reads the config from an account the program owns and ignores one it does not', async () => {
    const address = new PublicKey(fixture.config_address);
    const owned = new MemoryConnection().set(address, program, bytes);
    expect((await readCreatorCommitmentConfig(owned.asConnection(), mint, program))?.lockedTotal).toBe(9_000n);
    const foreign = new MemoryConnection().set(address, new PublicKey(Buffer.alloc(32, 9)), bytes);
    expect(await readCreatorCommitmentConfig(foreign.asConnection(), mint, program)).toBeNull();
  });
});

describe('vesting view and the sale preview', () => {
  const config = decodeCreatorCommitmentConfig(bytes);

  it('reports the phase and the share unlocked', () => {
    const start = config.start;
    expect(vestingView(config, start - 5n)).toMatchObject({ phase: 'before-cliff', locked: 9_000n, unlocked: 0n, elapsed: 0 });
    expect(vestingView(config, config.cliff).phase).toBe('vesting');
    const done = vestingView(config, config.end);
    expect(done).toMatchObject({ phase: 'complete', locked: 0n, unlocked: 9_000n });
    expect(done.elapsed).toBe(1);
  });

  it('flags a sale that would leave the account below the floor, and one that would not', () => {
    const creator = config.creatorAccount;
    const during = config.start + 10n;
    // balance 10,000 and 9,000 locked: selling 1,000 leaves exactly the floor
    expect(previewCreatorTransfer(config, creator, 10_000n, 1_000n, during)).toMatchObject({ violated: false, locked: 9_000n });
    expect(previewCreatorTransfer(config, creator, 10_000n, 1_001n, during)?.violated).toBe(true);
    // after the schedule everything may leave
    expect(previewCreatorTransfer(config, creator, 10_000n, 10_000n, config.end)?.violated).toBe(false);
    // selling more than the balance is a violation, not a negative number
    expect(previewCreatorTransfer(config, creator, 10n, 11n, config.end)?.violated).toBe(true);
  });

  it('says nothing about transfers from any other account', () => {
    expect(previewCreatorTransfer(config, new PublicKey(Buffer.alloc(32, 7)), 10_000n, 10_000n, config.start)).toBeNull();
  });
});
