import { generateKeyPairSync } from 'node:crypto';

/**
 * The end-to-end files avoid @solana/web3.js on purpose: Playwright's own TypeScript loader cannot load
 * its websocket dependency. A throwaway key and plain JSON-RPC are all the setup needs.
 */
export const RPC_URL = 'http://127.0.0.1:8899';

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

export async function rpc<T>(method: string, params: unknown[]): Promise<T> {
  const response = await fetch(RPC_URL, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }),
  });
  const body = (await response.json()) as { result?: T; error?: { message: string } };
  if (body.error) throw new Error(`${method}: ${body.error.message}`);
  return body.result as T;
}

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
  } catch {
    return 0n;
  }
}

export async function signatureCount(address: string): Promise<number> {
  const result = await rpc<unknown[]>('getSignaturesForAddress', [address, { limit: 1000, commitment: 'confirmed' }]);
  return result.length;
}
