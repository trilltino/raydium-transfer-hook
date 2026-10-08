import { getMint } from '@solana/spl-token';
import type { Connection, PublicKey } from '@solana/web3.js';

export interface PoolTokenInfo {
  mint: PublicKey;
  tokenProgram: PublicKey;
  decimals: number;
}

/**
 * The tokens (by mint address) that `wallet` is the mint authority of. Whoever created a token can mint more of
 * it from their own wallet, with no server key involved; nobody else can, and a mint whose authority was given
 * up cannot be minted at all.
 */
export async function mintedByWallet(connection: Connection, wallet: PublicKey, tokens: readonly PoolTokenInfo[]): Promise<Set<string>> {
  const own = new Set<string>();
  for (const token of tokens) {
    try {
      const mint = await getMint(connection, token.mint, 'confirmed', token.tokenProgram);
      if (mint.mintAuthority?.equals(wallet)) own.add(token.mint.toBase58());
    } catch {
      // A mint that cannot be read is not one this wallet can mint.
    }
  }
  return own;
}
