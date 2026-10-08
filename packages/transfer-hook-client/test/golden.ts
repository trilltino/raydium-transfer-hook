import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { type AccountMeta, PublicKey } from '@solana/web3.js';
import type { ResolvedLeg, TransferLeg } from '../src/index.ts';

/**
 * The Rust crate's committed golden files are the ABI record both languages agree on. Rust regenerates
 * them with `UPDATE_GOLDEN=1` and fails if they drift; these tests fail if the TypeScript builders do.
 */
const GOLDEN_DIR = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'crates', 'transfer-hook-sdk', 'tests', 'golden');

export interface GoldenAccount {
  index: number;
  role: string;
  pubkey: string;
  signer: boolean;
  writable: boolean;
}

export interface Golden {
  name: string;
  program_id: string;
  discriminator_hex: string;
  data_hex: string;
  accounts: GoldenAccount[];
}

export function loadGolden(file: string): Golden {
  return JSON.parse(readFileSync(join(GOLDEN_DIR, file), 'utf8')) as Golden;
}

export const keyOf = (golden: Golden, role: string): PublicKey => {
  const found = golden.accounts.find((account) => account.role === role);
  if (!found) throw new Error(`golden ${golden.name} has no ${role}`);
  return new PublicKey(found.pubkey);
};

export const metaOf = (account: GoldenAccount): AccountMeta => ({
  pubkey: new PublicKey(account.pubkey),
  isSigner: account.signer,
  isWritable: account.writable,
});

/** The hook slice a golden carries for `prefix` (`input` or `output`), in order. */
export function sliceOf(golden: Golden, prefix: 'input' | 'output'): AccountMeta[] {
  return golden.accounts
    .filter((account) => account.role.startsWith(`${prefix}_hook_`) || account.role === `${prefix}_validation_list`)
    .map(metaOf);
}

export function legOf(role: 'input' | 'output', fields: Omit<TransferLeg, 'role'>, slice: AccountMeta[]): ResolvedLeg {
  return { leg: { role, ...fields }, hookProgram: slice.length ? slice[slice.length - 2].pubkey : null, slice };
}

export const hex = (data: Uint8Array): string => Buffer.from(data).toString('hex');

export const u64 = (data: Buffer, offset: number): bigint => data.readBigUInt64LE(offset);
