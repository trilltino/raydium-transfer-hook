// Test-token faucet shared by `scripts/fund-wallet.mjs` and the dev server's /faucet route.
//
// The tokens of our pools are Token-2022 mints whose mint authority is the key that built the pool (the
// deployer on devnet, the committed fixture admin on a local validator). Only that key can create tokens, so
// the faucet is a server-side thing: this module runs in Node, never in the browser, and its key never
// reaches a page. Minting is not a transfer, so a Transfer Hook does not run.

import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  TOKEN_2022_PROGRAM_ID,
  createAssociatedTokenAccountIdempotentInstruction,
  createMintToInstruction,
  getAssociatedTokenAddressSync,
  getMint,
} from '@solana/spl-token';
import { Connection, Keypair, PublicKey, Transaction, sendAndConfirmTransaction } from '@solana/web3.js';

const here = dirname(fileURLToPath(import.meta.url));
export const repoRoot = resolve(here, '..', '..', '..');

/** Never mint more than this many whole tokens of one mint in one request. */
export const MAX_TOKENS = 1000n;
export const DEFAULT_TOKENS = 100n;

export function readKeypair(path) {
  return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync(path, 'utf8'))));
}

/**
 * What the faucet can do for a cluster, from the environment variables (`.env.local`): `devnet` needs
 * `FAUCET_KEYPAIR` (the mint authority's keypair file) and uses `FAUCET_RPC_URL`, else our Triton URL, else the
 * public devnet RPC. `localnet` needs nothing: it uses the fixture admin that built the local pools.
 */
export function faucetFor(cluster, env) {
  if (cluster === 'localnet') {
    return {
      keypairPath: resolve(repoRoot, 'tests', 'fixtures', 'localnet', 'admin.json'),
      rpcUrl: env.FAUCET_LOCAL_RPC_URL || 'http://127.0.0.1:8899',
    };
  }
  if (cluster === 'devnet' && env.FAUCET_KEYPAIR) {
    return {
      keypairPath: resolve(repoRoot, env.FAUCET_KEYPAIR),
      rpcUrl: env.FAUCET_RPC_URL || env.TRITON_DEVNET_RPC_URL || 'https://api.devnet.solana.com',
    };
  }
  return null;
}

/** The two mints of a CPMM or a CLMM pool, read from the pool account (the layouts of the forked programs). */
export async function readPoolMints(connection, pool, programs) {
  const info = await connection.getAccountInfo(new PublicKey(pool), 'confirmed');
  if (!info) throw new Error(`pool ${pool} does not exist on this cluster`);
  const owner = info.owner.toBase58();
  const offsets = owner === programs.cpmm ? [168, 200] : owner === programs.clmm ? [73, 105] : null;
  if (!offsets) throw new Error(`pool ${pool} is owned by ${owner}, which is not one of this environment's Raydium programs`);
  return offsets.map((offset) => new PublicKey(info.data.subarray(offset, offset + 32)));
}

/**
 * Create the wallet's token accounts (if needed) and mint `tokens` whole tokens of each mint into them. A mint
 * whose authority is not the faucet key is skipped with the reason; nothing is minted for it.
 */
export async function fundWallet({ connection, authority, wallet, mints, tokens = DEFAULT_TOKENS }) {
  if (tokens < 1n || tokens > MAX_TOKENS) throw new Error(`ask for between 1 and ${MAX_TOKENS} tokens`);
  const owner = new PublicKey(wallet);
  const instructions = [];
  const results = [];
  for (const mintKey of mints.map((mint) => new PublicKey(mint))) {
    const mint = await getMint(connection, mintKey, 'confirmed', TOKEN_2022_PROGRAM_ID).catch(() => null);
    if (!mint) {
      results.push({ mint: mintKey.toBase58(), status: 'skipped', reason: 'not a Token-2022 mint on this cluster' });
      continue;
    }
    if (!mint.mintAuthority || !mint.mintAuthority.equals(authority.publicKey)) {
      results.push({
        mint: mintKey.toBase58(),
        status: 'skipped',
        reason: mint.mintAuthority ? `its mint authority is ${mint.mintAuthority.toBase58()}, not the faucet key` : 'its mint authority has been given up',
      });
      continue;
    }
    const account = getAssociatedTokenAddressSync(mintKey, owner, true, TOKEN_2022_PROGRAM_ID);
    const amount = tokens * 10n ** BigInt(mint.decimals);
    instructions.push(
      createAssociatedTokenAccountIdempotentInstruction(authority.publicKey, account, owner, mintKey, TOKEN_2022_PROGRAM_ID),
      createMintToInstruction(mintKey, account, authority.publicKey, amount, [], TOKEN_2022_PROGRAM_ID)
    );
    results.push({ mint: mintKey.toBase58(), status: 'minted', account: account.toBase58(), amount: amount.toString(), decimals: mint.decimals });
  }
  let signature = null;
  if (instructions.length > 0) {
    signature = await sendAndConfirmTransaction(connection, new Transaction().add(...instructions), [authority], { commitment: 'confirmed' });
  }
  return { signature, results };
}

export function connectionFor(config) {
  return new Connection(config.rpcUrl, 'confirmed');
}
