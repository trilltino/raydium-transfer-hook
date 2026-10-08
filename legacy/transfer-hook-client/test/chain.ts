import { TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID, getExtraAccountMetaAddress } from '@solana/spl-token';
import { type AccountInfo, type Connection, Keypair, PublicKey } from '@solana/web3.js';
import { BPF_LOADER_UPGRADEABLE } from '../src/index.ts';

/** Fixed keys so every test reads the same. */
export const key = (byte: number): PublicKey => new PublicKey(Buffer.alloc(32, byte));

const EXECUTE_DISCRIMINATOR = Buffer.from([105, 37, 101, 197, 75, 251, 102, 26]);

/** A Token-2022 mint with a TransferHook extension (or a plain one when `hookProgram` is null). */
export function token2022MintData(hookProgram: PublicKey | null, authority: PublicKey = key(0xa0), decimals = 6): Buffer {
  const mint = Buffer.alloc(82);
  mint.writeUInt32LE(1, 0); // mint authority: Some
  key(0xaa).toBuffer().copy(mint, 4);
  mint.writeBigUInt64LE(1_000_000n, 36);
  mint[44] = decimals;
  mint[45] = 1; // initialised
  const base = Buffer.concat([mint, Buffer.alloc(165 - 82), Buffer.from([1])]);
  if (!hookProgram) return base;
  const tlv = Buffer.alloc(4 + 64);
  tlv.writeUInt16LE(14, 0); // ExtensionType::TransferHook
  tlv.writeUInt16LE(64, 2);
  const value = Buffer.concat([authority.toBuffer(), hookProgram.toBuffer()]);
  return Buffer.concat([base, tlv.subarray(0, 4), value]);
}

export function classicMintData(decimals = 6): Buffer {
  const mint = Buffer.alloc(82);
  mint[44] = decimals;
  mint[45] = 1;
  return mint;
}

export interface LiteralExtra {
  pubkey: PublicKey;
  isSigner?: boolean;
  isWritable?: boolean;
}

/** An ExtraAccountMetaList account holding literal-address extras. */
export function validationListData(extras: readonly LiteralExtra[]): Buffer {
  const header = Buffer.alloc(8 + 4 + 4);
  EXECUTE_DISCRIMINATOR.copy(header, 0);
  header.writeUInt32LE(4 + 35 * extras.length, 8);
  header.writeUInt32LE(extras.length, 12);
  const metas = extras.map((extra) => {
    const meta = Buffer.alloc(35);
    meta[0] = 0; // literal address
    extra.pubkey.toBuffer().copy(meta, 1);
    meta[33] = extra.isSigner ? 1 : 0;
    meta[34] = extra.isWritable ? 1 : 0;
    return meta;
  });
  return Buffer.concat([header, ...metas]);
}

type Entry = Pick<AccountInfo<Buffer>, 'owner' | 'data' | 'executable' | 'lamports'>;

/** An in-memory stand-in for the parts of `Connection` the client reads. */
export class MemoryConnection {
  private readonly accounts = new Map<string, Entry>();

  set(address: PublicKey, owner: PublicKey, data: Buffer, executable = false): this {
    this.accounts.set(address.toBase58(), { owner, data, executable, lamports: 1_000_000 });
    return this;
  }

  remove(address: PublicKey): this {
    this.accounts.delete(address.toBase58());
    return this;
  }

  addClassicMint(mint: PublicKey): this {
    return this.set(mint, TOKEN_PROGRAM_ID, classicMintData());
  }

  addProgram(program: PublicKey): this {
    return this.set(program, BPF_LOADER_UPGRADEABLE, Buffer.alloc(36), true);
  }

  /** A hooked Token-2022 mint, its hook program, and a validation list with `extras`. */
  addHookedMint(mint: PublicKey, hookProgram: PublicKey, extras: readonly LiteralExtra[]): this {
    this.set(mint, TOKEN_2022_PROGRAM_ID, token2022MintData(hookProgram));
    this.addProgram(hookProgram);
    return this.set(getExtraAccountMetaAddress(mint, hookProgram), hookProgram, validationListData(extras));
  }

  async getAccountInfo(address: PublicKey): Promise<AccountInfo<Buffer> | null> {
    const entry = this.accounts.get(address.toBase58());
    return entry ? { ...entry, rentEpoch: 0 } : null;
  }

  asConnection(): Connection {
    return this as unknown as Connection;
  }
}

export const fresh = (): PublicKey => Keypair.generate().publicKey;
