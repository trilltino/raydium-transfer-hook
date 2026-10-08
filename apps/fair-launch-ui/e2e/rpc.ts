import { generateKeyPairSync } from 'node:crypto';

/**
 * The end-to-end files avoid @solana/web3.js on purpose: Playwright's own TypeScript loader cannot load
 * its websocket dependency. A throwaway key and plain JSON-RPC are all the setup needs.
 */
export const DEVNET = process.env.E2E_ENV === 'devnet';
/** The cluster the test runs against: the local validator, or our integration devnet (`E2E_ENV=devnet`). */
export const RPC_URL = DEVNET ? 'https://api.devnet.solana.com' : 'http://127.0.0.1:8899';
/** The environment name the page selects with `?env=`. */
export const ENV_NAME = DEVNET ? 'integration-devnet' : 'localnet';

const ALPHABET = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

export function base58(bytes: Uint8Array): string {
  let value = 0n;
  for (const byte of bytes) value = (value << 8n) | BigInt(byte);
  let out = '';
  while (value > 0n) {
    out = ALPHABET[Number(value % 58n)] + out;
    value /= 58n;
  }
  for (const byte of bytes) {
    if (byte !== 0) break;
    out = `1${out}`;
  }
  return out;
}

/** A fresh ed25519 key in Solana's 64-byte secret form (seed then public key), and its address. */
export function newWallet(): { secret: number[]; address: string } {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const seed = privateKey.export({ format: 'der', type: 'pkcs8' }).subarray(-32);
  const pub = publicKey.export({ format: 'der', type: 'spki' }).subarray(-32);
  return { secret: [...seed, ...pub], address: base58(pub) };
}

/** Plain JSON-RPC. Public RPC endpoints rate-limit, so a 429 or a dropped connection is retried with a pause. */
export async function rpc<T>(method: string, params: unknown[]): Promise<T> {
  let lastError: unknown;
  for (let attempt = 0; attempt < 8; attempt += 1) {
    try {
      const response = await fetch(RPC_URL, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }),
      });
      if (response.status === 429 || response.status >= 500) throw new Error(`HTTP ${response.status}`);
      const body = (await response.json()) as { result?: T; error?: { message: string } };
      if (body.error) throw new RpcError(`${method}: ${body.error.message}`);
      return body.result as T;
    } catch (error) {
      if (error instanceof RpcError) throw error;
      lastError = error;
      await new Promise((resolve) => setTimeout(resolve, 1000 * (attempt + 1)));
    }
  }
  throw lastError;
}

/** An error the node answered with (as opposed to a transport failure, which is retried). */
class RpcError extends Error {}

export async function reachable(): Promise<boolean> {
  try {
    await rpc('getVersion', []);
    return true;
  } catch {
    return false;
  }
}

/** Raw token balance of a token account, or 0 if it does not exist yet. */
export async function tokenBalance(account: string): Promise<bigint> {
  try {
    const result = await rpc<{ value: { amount: string } }>('getTokenAccountBalance', [account, { commitment: 'confirmed' }]);
    return BigInt(result.value.amount);
  } catch (error) {
    // Only a missing account is a zero balance; anything else would hide a failed read.
    if (error instanceof Error && /could not find account|Invalid param/i.test(error.message)) return 0n;
    throw error;
  }
}

/** What `owner` holds of `mint` in all its token accounts (0 if it has none). */
export async function ownedBalance(owner: string, mint: string): Promise<bigint> {
  const result = await rpc<{ value: { account: { data: { parsed: { info: { tokenAmount: { amount: string } } } } } }[] }>(
    'getTokenAccountsByOwner',
    [owner, { mint }, { encoding: 'jsonParsed', commitment: 'confirmed' }]
  );
  return result.value.reduce((sum, entry) => sum + BigInt(entry.account.data.parsed.info.tokenAmount.amount), 0n);
}

/** Give an address SOL on the local validator (devnet has its own faucet) and wait until it shows. */
export async function airdrop(address: string, lamports: number): Promise<void> {
  await rpc<string>('requestAirdrop', [address, lamports]);
  for (let i = 0; i < 60; i += 1) {
    const { value } = await rpc<{ value: number }>('getBalance', [address, { commitment: 'confirmed' }]);
    if (value >= lamports) return;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error('the airdrop did not arrive');
}

export async function signatureCount(address: string): Promise<number> {
  const result = await rpc<unknown[]>('getSignaturesForAddress', [address, { limit: 1000, commitment: 'confirmed' }]);
  return result.length;
}
