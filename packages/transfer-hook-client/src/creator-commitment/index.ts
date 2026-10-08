import { type Commitment, type Connection, PublicKey } from '@solana/web3.js';
import { HookClientError } from '../hook/errors.ts';

/** A creator's allocation that vests: the schedule of one mint's commitment. Times are unix seconds. */
export interface CreatorCommitmentConfig {
  bump: number;
  mint: PublicKey;
  /** The token account whose balance may not fall below what is still locked. */
  creatorAccount: PublicKey;
  lockedTotal: bigint;
  start: bigint;
  cliff: bigint;
  end: bigint;
}

export const COMMITMENT_DISCRIMINATOR = Buffer.from('CRCONFIG', 'ascii');
export const COMMITMENT_CONFIG_LEN = 8 + 1 + 32 + 32 + 8 + 8 + 8 + 8;

/** The config PDA of `mint`: seeds `["config", mint]` under the creator-commitment program. */
export function getCreatorCommitmentConfigAddress(mint: PublicKey, programId: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync([Buffer.from('config'), mint.toBuffer()], programId)[0];
}

export function decodeCreatorCommitmentConfig(data: Uint8Array): CreatorCommitmentConfig {
  const buf = Buffer.from(data.buffer, data.byteOffset, data.byteLength);
  if (buf.length !== COMMITMENT_CONFIG_LEN || !buf.subarray(0, 8).equals(COMMITMENT_DISCRIMINATOR)) {
    throw new HookClientError('bad-instruction-input', 'not a creator-commitment config account');
  }
  return {
    bump: buf[8],
    mint: new PublicKey(buf.subarray(9, 41)),
    creatorAccount: new PublicKey(buf.subarray(41, 73)),
    lockedTotal: buf.readBigUInt64LE(73),
    start: buf.readBigInt64LE(81),
    cliff: buf.readBigInt64LE(89),
    end: buf.readBigInt64LE(97),
  };
}

export async function readCreatorCommitmentConfig(
  connection: Pick<Connection, 'getAccountInfo'>,
  mint: PublicKey,
  programId: PublicKey,
  commitment: Commitment = 'confirmed'
): Promise<CreatorCommitmentConfig | null> {
  const account = await connection.getAccountInfo(getCreatorCommitmentConfigAddress(mint, programId), commitment);
  if (account === null || !account.owner.equals(programId)) return null;
  return decodeCreatorCommitmentConfig(account.data);
}

/**
 * Tokens still locked at `now` (the program's `Schedule::locked_at`, rounding the same way): all of it
 * before the cliff, none from the end, and in between what the straight line from `start` to `end` has
 * not yet unlocked, rounded up so a token never unlocks early.
 */
export function lockedAt(config: Pick<CreatorCommitmentConfig, 'lockedTotal' | 'start' | 'cliff' | 'end'>, now: bigint): bigint {
  if (now < config.cliff) return config.lockedTotal;
  if (now >= config.end) return 0n;
  const unlocked = (config.lockedTotal * (now - config.start)) / (config.end - config.start);
  return config.lockedTotal - unlocked;
}

export type VestingPhase = 'before-cliff' | 'vesting' | 'complete';

export interface VestingView {
  phase: VestingPhase;
  locked: bigint;
  unlocked: bigint;
  /** 0..1 of the way from `start` to `end`. */
  elapsed: number;
}

export function vestingView(config: CreatorCommitmentConfig, now: bigint): VestingView {
  const locked = lockedAt(config, now);
  const span = Number(config.end - config.start);
  const done = Number(now - config.start);
  return {
    phase: now < config.cliff ? 'before-cliff' : now >= config.end ? 'complete' : 'vesting',
    locked,
    unlocked: config.lockedTotal - locked,
    elapsed: span <= 0 ? 1 : Math.min(1, Math.max(0, done / span)),
  };
}

/**
 * Whether sending `amount` out of the creator's account would break the floor: the program compares the
 * balance after the transfer with what is still locked. `null` if the transfer is not from that account.
 */
export function previewCreatorTransfer(
  config: CreatorCommitmentConfig,
  source: PublicKey,
  balance: bigint,
  amount: bigint,
  now: bigint
): { locked: bigint; balanceAfter: bigint; violated: boolean } | null {
  if (!source.equals(config.creatorAccount)) return null;
  const locked = lockedAt(config, now);
  const balanceAfter = balance >= amount ? balance - amount : 0n;
  return { locked, balanceAfter, violated: balance < amount || balanceAfter < locked };
}

export interface CommitmentErrorInfo {
  code: number;
  name: string;
  message: string;
  tradeRule: boolean;
}

/** The program's error codes (`templates/creator-commitment/src/error.rs`). */
export const COMMITMENT_ERRORS: readonly CommitmentErrorInfo[] = [
  { code: 0xa001, name: 'InvalidSchedule', message: 'The vesting schedule is invalid.', tradeRule: false },
  { code: 0xa002, name: 'ZeroLockedAmount', message: 'A commitment must lock something.', tradeRule: false },
  { code: 0xa003, name: 'CreatorAccountMismatch', message: 'The creator account is not a token account of this token.', tradeRule: false },
  { code: 0xa004, name: 'InsufficientBalanceAtInit', message: 'The creator account held less than it was meant to lock.', tradeRule: false },
  {
    code: 0xa005,
    name: 'VestingFloorBreached',
    message: 'This would leave the creator account below the amount still locked by its vesting schedule.',
    tradeRule: true,
  },
  { code: 0xa006, name: 'InvalidConfig', message: 'The commitment configuration account is not the expected one.', tradeRule: false },
  { code: 0xa007, name: 'InvalidInstruction', message: 'The commitment setup instruction was malformed.', tradeRule: false },
];

export function decodeCreatorCommitmentError(code: number): CommitmentErrorInfo | null {
  return COMMITMENT_ERRORS.find((info) => info.code === code) ?? null;
}
