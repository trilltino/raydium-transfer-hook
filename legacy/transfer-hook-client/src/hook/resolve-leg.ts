import { addExtraAccountMetasForExecute, getExtraAccountMetaAddress } from '@solana/spl-token';
import { type AccountMeta, type Commitment, type Connection, PublicKey, TransactionInstruction } from '@solana/web3.js';
import { HookClientError } from './errors.ts';
import { validateHookSlice } from './privileges.ts';
import { readTransferHook } from './read-hook.ts';

export type LegRole = 'input' | 'output';

/** One token transfer of a swap, as the swap performs it. */
export interface TransferLeg {
  role: LegRole;
  mint: PublicKey;
  source: PublicKey;
  destination: PublicKey;
  /** The account that signs the transfer: the trader for the input leg, the pool authority for the output leg. */
  authority: PublicKey;
  amount: bigint;
}

export interface ResolveOptions {
  /** Refuse a mint whose hook is any other program (so a mint re-pointed elsewhere is caught). */
  expectedHookProgram?: PublicKey;
  /** The only writable extras the integrator accepts. */
  allowWritable?: Iterable<PublicKey>;
  /** Loaders a hook program may be owned by. Defaults to the upgradeable, v2 and v4 loaders. */
  allowedLoaders?: readonly PublicKey[];
  commitment?: Commitment;
}

/** A leg with its hook slice: `extras..., hook program, validation list`, or empty if the mint has no hook. */
export interface ResolvedLeg {
  leg: TransferLeg;
  hookProgram: PublicKey | null;
  slice: AccountMeta[];
}

export const BPF_LOADER_UPGRADEABLE = new PublicKey('BPFLoaderUpgradeab1e11111111111111111111111');
export const BPF_LOADER_2 = new PublicKey('BPFLoader2111111111111111111111111111111111');
export const LOADER_V4 = new PublicKey('LoaderV411111111111111111111111111111111111');
export const DEFAULT_ALLOWED_LOADERS: readonly PublicKey[] = [BPF_LOADER_UPGRADEABLE, BPF_LOADER_2, LOADER_V4];

/**
 * Resolve one leg's Transfer Hook accounts. The mint's TransferHook extension decides whether there is a
 * hook; the validation list decides the extras (the SPL helper resolves them, including PDAs, so seed
 * logic is never reimplemented here); everything returned is then checked before it can be framed.
 */
export async function resolveTransferHookLeg(
  connection: Pick<Connection, 'getAccountInfo'>,
  leg: TransferLeg,
  options: ResolveOptions = {}
): Promise<ResolvedLeg> {
  const commitment = options.commitment ?? 'confirmed';
  const info = await readTransferHook(connection, leg.mint, commitment);
  if (info.hookProgramId === null) {
    if (options.expectedHookProgram) {
      throw new HookClientError(
        'hook-program-mismatch',
        `${leg.mint.toBase58()} has no hook but ${options.expectedHookProgram.toBase58()} was expected`
      );
    }
    return { leg, hookProgram: null, slice: [] };
  }
  const hookProgram = info.hookProgramId;
  if (options.expectedHookProgram && !hookProgram.equals(options.expectedHookProgram)) {
    throw new HookClientError(
      'hook-program-mismatch',
      `${leg.mint.toBase58()} points at hook ${hookProgram.toBase58()}, expected ${options.expectedHookProgram.toBase58()}`
    );
  }

  const program = await connection.getAccountInfo(hookProgram, commitment);
  const loaders = options.allowedLoaders ?? DEFAULT_ALLOWED_LOADERS;
  if (program === null || !program.executable || !loaders.some((loader) => loader.equals(program.owner))) {
    throw new HookClientError(
      'hook-program-invalid',
      `hook program ${hookProgram.toBase58()} is missing, not executable, or not owned by an allowed loader`
    );
  }
  const listAddress = getExtraAccountMetaAddress(leg.mint, hookProgram);
  const list = await connection.getAccountInfo(listAddress, commitment);
  if (list === null) {
    throw new HookClientError('validation-list-missing', `the hook ${hookProgram.toBase58()} has no validation list for ${leg.mint.toBase58()}`);
  }
  if (!list.owner.equals(hookProgram)) {
    throw new HookClientError('validation-list-owner', `the validation list ${listAddress.toBase58()} is not owned by the hook program`);
  }

  // The SPL helper appends `extras..., hook program, validation list` to an instruction that already
  // has the four transfer accounts, so the slice is everything after them.
  const probe = new TransactionInstruction({
    programId: info.tokenProgram,
    keys: [
      { pubkey: leg.source, isSigner: false, isWritable: true },
      { pubkey: leg.mint, isSigner: false, isWritable: false },
      { pubkey: leg.destination, isSigner: false, isWritable: true },
      { pubkey: leg.authority, isSigner: true, isWritable: false },
    ],
    data: Buffer.alloc(0),
  });
  await addExtraAccountMetasForExecute(
    connection as Connection,
    probe,
    hookProgram,
    leg.source,
    leg.mint,
    leg.destination,
    leg.authority,
    leg.amount,
    commitment
  );
  const slice = probe.keys.slice(4).map((meta) => ({ pubkey: meta.pubkey, isSigner: meta.isSigner, isWritable: meta.isWritable }));
  validateHookSlice(slice, {
    mint: leg.mint,
    hookProgram,
    allowWritable: new Set([...(options.allowWritable ?? [])].map((key) => key.toBase58())),
  });
  return { leg, hookProgram, slice };
}
