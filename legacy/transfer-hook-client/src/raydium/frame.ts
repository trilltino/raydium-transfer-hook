import { type AccountMeta, PublicKey } from '@solana/web3.js';
import { HookClientError } from '../hook/errors.ts';
import { assertNoPrivilegeConflict } from '../hook/privileges.ts';
import type { LegRole, ResolvedLeg } from '../hook/resolve-leg.ts';

/** Where a leg's four transfer accounts sit in a swap's fixed account list. */
export interface LegLayout {
  source: number;
  destination: number;
  authority: number;
  mint: number;
}

/** Check that the leg the slice was resolved for is the transfer the swap really performs. */
export function checkLegMatches(role: LegRole, leg: ResolvedLeg['leg'], layout: LegLayout, fixed: readonly AccountMeta[]): void {
  const fields: [string, number, PublicKey][] = [
    ['mint', layout.mint, leg.mint],
    ['source', layout.source, leg.source],
    ['destination', layout.destination, leg.destination],
    ['authority', layout.authority, leg.authority],
  ];
  for (const [field, index, found] of fields) {
    const expected = fixed[index].pubkey;
    if (!expected.equals(found)) {
      throw new HookClientError(
        'leg-mismatch',
        `the ${role} leg's ${field} is ${found.toBase58()} but the swap uses ${expected.toBase58()} there`
      );
    }
  }
}

export interface FramedAccounts {
  accounts: AccountMeta[];
  inputCount: number;
  outputCount: number;
}

/**
 * Check both legs against the fixed accounts and return `fixed ++ extra ++ input slice ++ output slice`.
 * `extra` is the CLMM tick arrays and bitmap; slices are never merged, sorted or deduplicated.
 */
export function frameAccounts(
  fixed: readonly AccountMeta[],
  extra: readonly AccountMeta[],
  input: ResolvedLeg,
  output: ResolvedLeg,
  inputLayout: LegLayout,
  outputLayout: LegLayout
): FramedAccounts {
  checkLegMatches('input', input.leg, inputLayout, fixed);
  checkLegMatches('output', output.leg, outputLayout, fixed);
  assertNoPrivilegeConflict([...fixed, ...extra], input.slice, output.slice);
  for (const count of [input.slice.length, output.slice.length]) {
    if (count > 0xffff) throw new HookClientError('bad-instruction-input', 'a hook slice has more than 65535 accounts');
  }
  return {
    accounts: [...fixed, ...extra, ...input.slice, ...output.slice],
    inputCount: input.slice.length,
    outputCount: output.slice.length,
  };
}

export function u64le(value: bigint): Buffer {
  if (value < 0n || value >= 1n << 64n) throw new HookClientError('bad-instruction-input', `${value} does not fit in a u64`);
  const out = Buffer.alloc(8);
  out.writeBigUInt64LE(value);
  return out;
}

export function u128le(value: bigint): Buffer {
  if (value < 0n || value >= 1n << 128n) throw new HookClientError('bad-instruction-input', `${value} does not fit in a u128`);
  const out = Buffer.alloc(16);
  out.writeBigUInt64LE(value & ((1n << 64n) - 1n), 0);
  out.writeBigUInt64LE(value >> 64n, 8);
  return out;
}

export function u16le(value: number): Buffer {
  if (!Number.isInteger(value) || value < 0 || value > 0xffff) {
    throw new HookClientError('bad-instruction-input', `${value} does not fit in a u16`);
  }
  const out = Buffer.alloc(2);
  out.writeUInt16LE(value);
  return out;
}
