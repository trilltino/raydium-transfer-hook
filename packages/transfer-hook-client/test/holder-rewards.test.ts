import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PublicKey } from '@solana/web3.js';
import { describe, expect, it } from 'vitest';
import {
  GLOBAL_LEN,
  RECORD_LEN,
  REWARDS_ERRORS,
  buildClaimInstruction,
  buildRegisterInstruction,
  claimableAt,
  decodeHolderRecord,
  decodeRewardsError,
  decodeRewardsGlobal,
  getHolderRecordAddress,
  getRewardVaultAddress,
  getRewardsGlobalAddress,
  indexAt,
  readHolderRecord,
  readRewardsGlobal,
  rewardsView,
  rewardsWritableExtras,
} from '../src/index.ts';
import { MemoryConnection } from './chain.ts';

interface InstructionFixture {
  data_hex: string;
  accounts: { pubkey: string; signer: boolean; writable: boolean }[];
}

/** Written by `templates/holder-rewards` (`UPDATE_GOLDEN=1 cargo test -p holder-rewards-hook typescript_fixture`). */
const fixture = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'tests', 'fixtures', 'typescript', 'holder-rewards.json'), 'utf8')
) as {
  program_id: string;
  mint: string;
  reward_mint: string;
  holder_account: string;
  global_address: string;
  reward_vault_address: string;
  record_address: string;
  global_hex: string;
  record_hex: string;
  claimable: { now: number; balance: number; claimable: number }[];
  register: { payer: string; instruction: InstructionFixture };
  claim: { owner: string; owner_reward_account: string; reward_token_program: string; instruction: InstructionFixture };
  errors: { name: string; code: number }[];
};

const program = new PublicKey(fixture.program_id);
const mint = new PublicKey(fixture.mint);
const holderAccount = new PublicKey(fixture.holder_account);
const globalBytes = Buffer.from(fixture.global_hex, 'hex');
const recordBytes = Buffer.from(fixture.record_hex, 'hex');

const shape = (ix: { data: Buffer; keys: { pubkey: PublicKey; isSigner: boolean; isWritable: boolean }[] }) => ({
  data: ix.data.toString('hex'),
  accounts: ix.keys.map((k) => ({ pubkey: k.pubkey.toBase58(), signer: k.isSigner, writable: k.isWritable })),
});

describe('the Rust holder-rewards fixture', () => {
  it('derives the same addresses', () => {
    expect(getRewardsGlobalAddress(mint, program).toBase58()).toBe(fixture.global_address);
    expect(getRewardVaultAddress(mint, program).toBase58()).toBe(fixture.reward_vault_address);
    expect(getHolderRecordAddress(holderAccount, program).toBase58()).toBe(fixture.record_address);
  });

  it('decodes the 186-byte global and the 73-byte record', () => {
    expect(globalBytes.length).toBe(GLOBAL_LEN);
    expect(recordBytes.length).toBe(RECORD_LEN);
    const global = decodeRewardsGlobal(globalBytes);
    expect(global.mint.toBase58()).toBe(fixture.mint);
    expect(global.rewardMint.toBase58()).toBe(fixture.reward_mint);
    expect(global.rewardVault.toBase58()).toBe(fixture.reward_vault_address);
    expect(global.oneTime).toBe(true);
    expect(global.stream.rate).toBe(2_000n);
    expect(global.stream.periodFinish).toBe(4_600n);
    expect(global.stream.eligibleSupply).toBe(400n);
    const record = decodeHolderRecord(recordBytes);
    expect(record.tokenAccount.toBase58()).toBe(fixture.holder_account);
    expect(record.checkpoint).toBe(100n);
  });

  it('computes what a record could claim exactly as the program does, at every sampled time and balance', () => {
    const global = decodeRewardsGlobal(globalBytes);
    const record = decodeHolderRecord(recordBytes);
    for (const sample of fixture.claimable) {
      expect(claimableAt(global, record, BigInt(sample.balance), BigInt(sample.now)), `at ${sample.now} with ${sample.balance}`).toBe(
        BigInt(sample.claimable)
      );
    }
  });

  it('builds Register and Claim byte for byte, in the program account order and with its flags', () => {
    const payer = new PublicKey(fixture.register.payer);
    expect(shape(buildRegisterInstruction(program, payer, mint, holderAccount))).toEqual({
      data: fixture.register.instruction.data_hex,
      accounts: fixture.register.instruction.accounts,
    });
    const claim = buildClaimInstruction(
      program,
      new PublicKey(fixture.claim.owner),
      mint,
      holderAccount,
      new PublicKey(fixture.claim.owner_reward_account),
      new PublicKey(fixture.reward_mint),
      new PublicKey(fixture.claim.reward_token_program)
    );
    expect(shape(claim)).toEqual({ data: fixture.claim.instruction.data_hex, accounts: fixture.claim.instruction.accounts });
  });

  it('knows every error code the template defines', () => {
    expect(REWARDS_ERRORS.map(({ name, code }) => ({ name, code })).sort((a, b) => a.code - b.code)).toEqual(
      [...fixture.errors].sort((a, b) => a.code - b.code)
    );
    expect(decodeRewardsError(0xc005)?.name).toBe('NotRegistered');
    expect(decodeRewardsError(0xa005)).toBeNull();
  });

  it('refuses malformed accounts and a bad mode byte', () => {
    expect(() => decodeRewardsGlobal(globalBytes.subarray(0, GLOBAL_LEN - 1))).toThrow();
    const badMode = Buffer.from(globalBytes);
    badMode[185] = 2;
    expect(() => decodeRewardsGlobal(badMode)).toThrow();
    const wrongTag = Buffer.from(recordBytes);
    wrongTag[0] ^= 1;
    expect(() => decodeHolderRecord(wrongTag)).toThrow();
  });

  it('reads the global and a record only from accounts the program owns', async () => {
    const owned = new MemoryConnection()
      .set(new PublicKey(fixture.global_address), program, globalBytes)
      .set(new PublicKey(fixture.record_address), program, recordBytes);
    expect((await readRewardsGlobal(owned.asConnection(), mint, program))?.stream.rate).toBe(2_000n);
    expect((await readHolderRecord(owned.asConnection(), holderAccount, program))?.checkpoint).toBe(100n);
    // an account nobody registered has no record
    expect(await readHolderRecord(owned.asConnection(), new PublicKey(Buffer.alloc(32, 3)), program)).toBeNull();
    const foreign = new MemoryConnection().set(new PublicKey(fixture.record_address), new PublicKey(Buffer.alloc(32, 9)), recordBytes);
    expect(await readHolderRecord(foreign.asConnection(), holderAccount, program)).toBeNull();
  });
});

describe('the stream view', () => {
  const global = decodeRewardsGlobal(globalBytes);

  it('stops the index at the end of the funded period and does not run backwards', () => {
    expect(indexAt(global.stream, 4_600n)).toBe(indexAt(global.stream, 9_999n));
    expect(indexAt(global.stream, 900n)).toBe(global.stream.index);
  });

  it('shows the remaining time, the rate and the account share', () => {
    expect(rewardsView(global, 100n, true, 1_600n)).toMatchObject({ funded: true, secondsLeft: 3_000n, ratePerSecond: 2_000n, sharePpm: 250_000n });
    expect(rewardsView(global, 100n, false, 1_600n).sharePpm).toBe(0n);
    expect(rewardsView(global, 100n, true, 5_000n)).toMatchObject({ secondsLeft: 0n, ratePerSecond: 0n });
  });

  it('names the three writable extras of a hooked transfer', () => {
    const source = new PublicKey(Buffer.alloc(32, 5));
    const destination = new PublicKey(Buffer.alloc(32, 6));
    expect(rewardsWritableExtras(mint, source, destination, program).map((key) => key.toBase58())).toEqual([
      getRewardsGlobalAddress(mint, program).toBase58(),
      getHolderRecordAddress(source, program).toBase58(),
      getHolderRecordAddress(destination, program).toBase58(),
    ]);
  });
});
