import { getExtraAccountMetaAddress } from '@solana/spl-token';
import { type AccountMeta, PublicKey } from '@solana/web3.js';
import { HookClientError } from './errors.ts';

export interface SliceRules {
  mint: PublicKey;
  hookProgram: PublicKey;
  /** The only writable extra accounts the integrator accepts, by base58 address (the hook's own state). */
  allowWritable: ReadonlySet<string>;
}

const same = (a: AccountMeta, b: AccountMeta): boolean => a.pubkey.equals(b.pubkey);

/**
 * Check one hook slice: the extras the validation list resolved, then the hook program, then the
 * validation list. Extras may never sign, and may be writable only if the caller named them;
 * the tail must be exactly the hook program and the canonical validation list, both read-only.
 */
export function validateHookSlice(slice: readonly AccountMeta[], rules: SliceRules): void {
  if (slice.length < 2) {
    throw new HookClientError('bad-slice-tail', 'a hook slice has at least the hook program and the validation list');
  }
  const program = slice[slice.length - 2];
  const list = slice[slice.length - 1];
  if (!program.pubkey.equals(rules.hookProgram) || program.isSigner || program.isWritable) {
    throw new HookClientError('bad-slice-tail', 'the second-to-last account of a slice must be the read-only hook program');
  }
  const canonical = getExtraAccountMetaAddress(rules.mint, rules.hookProgram);
  if (!list.pubkey.equals(canonical) || list.isSigner || list.isWritable) {
    throw new HookClientError('bad-slice-tail', 'the last account of a slice must be the read-only canonical validation list');
  }
  for (const extra of slice.slice(0, -2)) {
    if (extra.isSigner) {
      throw new HookClientError('unexpected-signer', `the hook asked for ${extra.pubkey.toBase58()} to sign; hook accounts never sign`);
    }
    if (extra.isWritable && !rules.allowWritable.has(extra.pubkey.toBase58())) {
      throw new HookClientError(
        'unexpected-writable',
        `the hook asked for ${extra.pubkey.toBase58()} to be writable and it was not named as allowed`
      );
    }
  }
}

/**
 * Solana merges account flags per key across a transaction, so a slice account that shares a key with
 * a fixed account, or with the other slice, and carries more privilege would escalate that account for
 * Raydium's own handler. Refuse such a transaction.
 */
export function assertNoPrivilegeConflict(
  fixed: readonly AccountMeta[],
  input: readonly AccountMeta[],
  output: readonly AccountMeta[]
): void {
  for (const [name, slice] of [['input', input], ['output', output]] as const) {
    for (const meta of slice) {
      for (const fixedMeta of fixed) {
        if (same(meta, fixedMeta) && ((meta.isSigner && !fixedMeta.isSigner) || (meta.isWritable && !fixedMeta.isWritable))) {
          throw new HookClientError(
            'privilege-conflict',
            `the ${name} hook slice would escalate ${meta.pubkey.toBase58()}, which the swap already lists with fewer privileges`
          );
        }
      }
    }
  }
  for (const a of input) {
    for (const b of output) {
      if (same(a, b) && (a.isSigner !== b.isSigner || a.isWritable !== b.isWritable)) {
        throw new HookClientError('privilege-conflict', `${a.pubkey.toBase58()} appears in both hook slices with different privileges`);
      }
    }
  }
}

export type { PublicKey };
