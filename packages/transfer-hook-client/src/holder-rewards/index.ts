import { type Commitment, type Connection, PublicKey, SystemProgram, TransactionInstruction } from '@solana/web3.js';
import { HookClientError } from '../hook/errors.ts';

/** Fixed-point scale of the reward index (`rule::PRECISION`). */
export const REWARD_PRECISION = 1_000_000_000_000n;

export const GLOBAL_DISCRIMINATOR = Buffer.from('LRGLOBAL', 'ascii');
export const GLOBAL_LEN = 8 + 1 + 32 * 4 + 8 + 8 + 8 + 16 + 8 + 1;
export const RECORD_DISCRIMINATOR = Buffer.from('LRHOLDER', 'ascii');
export const RECORD_LEN = 8 + 1 + 32 + 8 + 16 + 8;

/** The reward pool's running accounts, as the program stores them. */
export interface RewardStream {
  /** Reward tokens paid out per second while the period lasts. */
  rate: bigint;
  periodFinish: bigint;
  lastUpdate: bigint;
  /** Reward earned so far per unit of balance, scaled by {@link REWARD_PRECISION}. */
  index: bigint;
  /** The sum of the balances of all registered accounts. */
  eligibleSupply: bigint;
}

export interface HolderRewardsGlobal {
  bump: number;
  mint: PublicKey;
  rewardMint: PublicKey;
  rewardVault: PublicKey;
  /** The pool's vault of the hooked token: it can never register, so it never earns. */
  poolVault: PublicKey;
  stream: RewardStream;
  /** A one-time allocation (a spin-off): it can be funded once. Otherwise it can be topped up. */
  oneTime: boolean;
}

/** One registered token account's place in the stream. */
export interface HolderRecord {
  bump: number;
  tokenAccount: PublicKey;
  /** The balance this record last counted in the eligible supply. */
  checkpoint: bigint;
  /** The index value this holder was last settled at. */
  indexPaid: bigint;
  /** Earned and settled, not yet claimed. */
  earned: bigint;
}

const seed = (text: string): Buffer => Buffer.from(text, 'utf8');

/** The global PDA of `mint` (it also owns the reward vault): seeds `["rewards", mint]`. */
export const getRewardsGlobalAddress = (mint: PublicKey, programId: PublicKey): PublicKey =>
  PublicKey.findProgramAddressSync([seed('rewards'), mint.toBuffer()], programId)[0];

/** The reward vault of `mint`: seeds `["reward-vault", mint]`. */
export const getRewardVaultAddress = (mint: PublicKey, programId: PublicKey): PublicKey =>
  PublicKey.findProgramAddressSync([seed('reward-vault'), mint.toBuffer()], programId)[0];

/** The holder record of a token account: seeds `["holder", token_account]`. */
export const getHolderRecordAddress = (tokenAccount: PublicKey, programId: PublicKey): PublicKey =>
  PublicKey.findProgramAddressSync([seed('holder'), tokenAccount.toBuffer()], programId)[0];

const view = (data: Uint8Array): Buffer => Buffer.from(data.buffer, data.byteOffset, data.byteLength);

export function decodeRewardsGlobal(data: Uint8Array): HolderRewardsGlobal {
  const buf = view(data);
  if (buf.length !== GLOBAL_LEN || !buf.subarray(0, 8).equals(GLOBAL_DISCRIMINATOR) || buf[185] > 1) {
    throw new HookClientError('bad-instruction-input', 'not a holder-rewards global account');
  }
  const u128 = (offset: number): bigint => buf.readBigUInt64LE(offset) | (buf.readBigUInt64LE(offset + 8) << 64n);
  return {
    bump: buf[8],
    mint: new PublicKey(buf.subarray(9, 41)),
    rewardMint: new PublicKey(buf.subarray(41, 73)),
    rewardVault: new PublicKey(buf.subarray(73, 105)),
    poolVault: new PublicKey(buf.subarray(105, 137)),
    stream: {
      rate: buf.readBigUInt64LE(137),
      periodFinish: buf.readBigInt64LE(145),
      lastUpdate: buf.readBigInt64LE(153),
      index: u128(161),
      eligibleSupply: buf.readBigUInt64LE(177),
    },
    oneTime: buf[185] === 1,
  };
}

export function decodeHolderRecord(data: Uint8Array): HolderRecord {
  const buf = view(data);
  if (buf.length !== RECORD_LEN || !buf.subarray(0, 8).equals(RECORD_DISCRIMINATOR)) {
    throw new HookClientError('bad-instruction-input', 'not a holder-rewards record account');
  }
  return {
    bump: buf[8],
    tokenAccount: new PublicKey(buf.subarray(9, 41)),
    checkpoint: buf.readBigUInt64LE(41),
    indexPaid: buf.readBigUInt64LE(49) | (buf.readBigUInt64LE(57) << 64n),
    earned: buf.readBigUInt64LE(65),
  };
}

export async function readRewardsGlobal(
  connection: Pick<Connection, 'getAccountInfo'>,
  mint: PublicKey,
  programId: PublicKey,
  commitment: Commitment = 'confirmed'
): Promise<HolderRewardsGlobal | null> {
  const account = await connection.getAccountInfo(getRewardsGlobalAddress(mint, programId), commitment);
  if (account === null || !account.owner.equals(programId)) return null;
  return decodeRewardsGlobal(account.data);
}

/** The holder record of a token account; `null` if that account never registered. */
export async function readHolderRecord(
  connection: Pick<Connection, 'getAccountInfo'>,
  tokenAccount: PublicKey,
  programId: PublicKey,
  commitment: Commitment = 'confirmed'
): Promise<HolderRecord | null> {
  const account = await connection.getAccountInfo(getHolderRecordAddress(tokenAccount, programId), commitment);
  if (account === null || !account.owner.equals(programId)) return null;
  return decodeHolderRecord(account.data);
}

/** The index at `now`: the program's `Stream::advance`, integer arithmetic that rounds down. */
export function indexAt(stream: RewardStream, now: bigint): bigint {
  const until = now < stream.periodFinish ? now : stream.periodFinish;
  if (until > stream.lastUpdate && stream.eligibleSupply > 0n) {
    return stream.index + (stream.rate * (until - stream.lastUpdate) * REWARD_PRECISION) / stream.eligibleSupply;
  }
  return stream.index;
}

/**
 * What a registered account could claim at `now`, holding `balance`: what is already settled plus what
 * accrued since (`Holder::settle`: the smaller of the counted and the actual balance times the growth of
 * the index). It is what `Claim` pays, to the unit, if no one else's transfer changes the stream first.
 */
export function claimableAt(global: HolderRewardsGlobal, record: HolderRecord, balance: bigint, now: bigint): bigint {
  const earning = record.checkpoint < balance ? record.checkpoint : balance;
  const accrued = (earning * (indexAt(global.stream, now) - record.indexPaid)) / REWARD_PRECISION;
  return record.earned + accrued;
}

export interface RewardsView {
  funded: boolean;
  /** Seconds until the funded period ends; 0 if it has. */
  secondsLeft: bigint;
  /** Reward tokens per second the whole stream pays now (0 once the period has ended). */
  ratePerSecond: bigint;
  /** This account's share of the stream, in parts per million of the eligible supply. */
  sharePpm: bigint;
}

export function rewardsView(global: HolderRewardsGlobal, balance: bigint, registered: boolean, now: bigint): RewardsView {
  const left = global.stream.periodFinish > now ? global.stream.periodFinish - now : 0n;
  return {
    funded: global.stream.rate > 0n,
    secondsLeft: left,
    ratePerSecond: left > 0n ? global.stream.rate : 0n,
    sharePpm: registered && global.stream.eligibleSupply > 0n ? (balance * 1_000_000n) / global.stream.eligibleSupply : 0n,
  };
}

/** `Register`: start counting `tokenAccount` in the stream. `payer` signs and pays the record's rent. */
export function buildRegisterInstruction(
  programId: PublicKey,
  payer: PublicKey,
  mint: PublicKey,
  tokenAccount: PublicKey
): TransactionInstruction {
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: payer, isSigner: true, isWritable: true },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: tokenAccount, isSigner: false, isWritable: false },
      { pubkey: getHolderRecordAddress(tokenAccount, programId), isSigner: false, isWritable: true },
      { pubkey: getRewardsGlobalAddress(mint, programId), isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.from([1]),
  });
}

/** `Claim`: `owner` (who must own `tokenAccount`) is paid what the account has earned, into `ownerRewardAccount`. */
export function buildClaimInstruction(
  programId: PublicKey,
  owner: PublicKey,
  mint: PublicKey,
  tokenAccount: PublicKey,
  ownerRewardAccount: PublicKey,
  rewardMint: PublicKey,
  rewardTokenProgram: PublicKey
): TransactionInstruction {
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: owner, isSigner: true, isWritable: false },
      { pubkey: tokenAccount, isSigner: false, isWritable: false },
      { pubkey: getHolderRecordAddress(tokenAccount, programId), isSigner: false, isWritable: true },
      { pubkey: getRewardsGlobalAddress(mint, programId), isSigner: false, isWritable: true },
      { pubkey: getRewardVaultAddress(mint, programId), isSigner: false, isWritable: true },
      { pubkey: ownerRewardAccount, isSigner: false, isWritable: true },
      { pubkey: rewardMint, isSigner: false, isWritable: false },
      { pubkey: rewardTokenProgram, isSigner: false, isWritable: false },
    ],
    data: Buffer.from([3]),
  });
}

/**
 * The writable extras a hooked transfer of `mint` between `source` and `destination` carries: the global
 * and the two accounts' records. A swap must name them, or the client refuses the hook's accounts.
 */
export function rewardsWritableExtras(mint: PublicKey, source: PublicKey, destination: PublicKey, programId: PublicKey): PublicKey[] {
  return [getRewardsGlobalAddress(mint, programId), getHolderRecordAddress(source, programId), getHolderRecordAddress(destination, programId)];
}

export interface RewardsErrorInfo {
  code: number;
  name: string;
  message: string;
}

/** The program's error codes (`templates/holder-rewards/src/error.rs`). */
export const REWARDS_ERRORS: readonly RewardsErrorInfo[] = [
  { code: 0xc001, name: 'InvalidInstruction', message: 'The instruction was malformed.' },
  { code: 0xc002, name: 'InvalidGlobal', message: 'The rewards account is not the one this token created.' },
  { code: 0xc003, name: 'InvalidRecord', message: 'The holder record is not the one for this token account.' },
  { code: 0xc004, name: 'ExcludedAccount', message: 'The pool’s own vault cannot earn rewards, so it cannot register.' },
  { code: 0xc005, name: 'NotRegistered', message: 'This token account is not registered for rewards. Register it first.' },
  { code: 0xc006, name: 'ZeroAmount', message: 'The amount was zero.' },
  { code: 0xc007, name: 'InvalidDuration', message: 'The reward period is zero or longer than allowed.' },
  { code: 0xc008, name: 'MathOverflow', message: 'A reward calculation overflowed.' },
  { code: 0xc009, name: 'RewardMintHasHook', message: 'The reward token has a Transfer Hook of its own, which rewards do not support.' },
  { code: 0xc00a, name: 'RewardAccountMismatch', message: 'The reward account is not one of the reward token.' },
  { code: 0xc00b, name: 'WrongOwner', message: 'Only the owner of the token account can do this.' },
  { code: 0xc00c, name: 'NothingToClaim', message: 'There is nothing to claim yet.' },
  { code: 0xc00d, name: 'PoolVaultMismatch', message: 'The pool vault is not a token account of this token.' },
  { code: 0xc00e, name: 'AlreadyFunded', message: 'This one-time allocation was already funded.' },
];

export function decodeRewardsError(code: number): RewardsErrorInfo | null {
  return REWARDS_ERRORS.find((info) => info.code === code) ?? null;
}
