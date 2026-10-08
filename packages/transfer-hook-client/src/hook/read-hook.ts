import { TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID, getTransferHook, unpackMint } from '@solana/spl-token';
import { type Commitment, type Connection, PublicKey } from '@solana/web3.js';
import { HookClientError } from './errors.ts';

export interface TransferHookInfo {
  mint: PublicKey;
  /** The token program that owns the mint. */
  tokenProgram: PublicKey;
  /** The mint's Transfer Hook program, or `null` if it has none (or none set yet). */
  hookProgramId: PublicKey | null;
  /** Who may re-point the mint at a different hook, or `null` once revoked. */
  hookAuthority: PublicKey | null;
  decimals: number;
}

/**
 * Read a mint's TransferHook extension. A classic SPL Token mint, a Token-2022 mint without the
 * extension, and one whose hook is not set yet all come back with `hookProgramId: null`; any other
 * owner is refused, because it is not a token mint at all.
 */
export async function readTransferHook(
  connection: Pick<Connection, 'getAccountInfo'>,
  mint: PublicKey,
  commitment: Commitment = 'confirmed'
): Promise<TransferHookInfo> {
  const account = await connection.getAccountInfo(mint, commitment);
  if (account === null) throw new HookClientError('mint-missing', `the mint ${mint.toBase58()} does not exist`);
  if (account.owner.equals(TOKEN_PROGRAM_ID)) {
    // The classic program has no extensions; decimals sits at byte 44 of the 82-byte mint.
    return { mint, tokenProgram: TOKEN_PROGRAM_ID, hookProgramId: null, hookAuthority: null, decimals: account.data[44] ?? 0 };
  }
  if (!account.owner.equals(TOKEN_2022_PROGRAM_ID)) {
    throw new HookClientError('unsupported-token-program', `${mint.toBase58()} is owned by ${account.owner.toBase58()}, not a token program`);
  }
  const state = unpackMint(mint, account, TOKEN_2022_PROGRAM_ID);
  const hook = getTransferHook(state);
  const none = (key: PublicKey): PublicKey | null => (key.equals(PublicKey.default) ? null : key);
  return {
    mint,
    tokenProgram: TOKEN_2022_PROGRAM_ID,
    hookProgramId: hook ? none(hook.programId) : null,
    hookAuthority: hook ? none(hook.authority) : null,
    decimals: state.decimals,
  };
}
