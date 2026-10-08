import { type HookEnvironment, readTransferHook } from '@raydium-transfer-hook/client';
import { PublicKey, type Connection } from '@solana/web3.js';

/** `sha256("account:SupportMintAssociated")[..8]`: what the pool admin's per-mint approval record starts with. */
const SUPPORT_MINT_DISCRIMINATOR = '8628b74f0c70a235';

export type Approval = 'approved' | 'not-approved' | 'invalid';

export interface TokenCheck {
  mint: string;
  /** `hooked`: a Token-2022 mint with a Transfer Hook; `plain`: a mint without one; `missing`: no such account; `invalid`: not an address, or not a token mint. */
  kind: 'hooked' | 'plain' | 'missing' | 'invalid';
  message: string;
  hookProgram?: string;
  /** Who may re-point the mint at another hook, `null` once revoked. */
  hookAuthority?: string | null;
  /** Whether the pool admin has approved the mint on each AMM (only checked for a hooked mint). */
  approval?: { cpmm: Approval; clmm: Approval };
}

const hex = (bytes: Uint8Array): string => [...bytes].map((byte) => byte.toString(16).padStart(2, '0')).join('');

function judge(account: { owner: PublicKey; executable: boolean; data: Uint8Array } | null, program: PublicKey, mint: PublicKey): Approval {
  if (account === null) return 'not-approved';
  if (account.executable || !account.owner.equals(program)) return 'invalid';
  // 8-byte discriminator, 1-byte bump, 32-byte mint.
  if (account.data.length < 41 || hex(account.data.subarray(0, 8)) !== SUPPORT_MINT_DISCRIMINATOR) return 'invalid';
  return hex(account.data.subarray(9, 41)) === hex(mint.toBytes()) ? 'approved' : 'invalid';
}

/**
 * What is true of a token before anyone tries to make a pool of it: is it a mint, does it have a Transfer Hook,
 * and has the pool admin approved it on each of our forked AMMs (the gate that decides whether a pool can exist).
 */
export async function checkToken(connection: Connection, environment: HookEnvironment, text: string): Promise<TokenCheck> {
  let mint: PublicKey;
  try {
    mint = new PublicKey(text.trim());
  } catch {
    return { mint: text.trim(), kind: 'invalid', message: 'That is not a valid address.' };
  }
  const address = mint.toBase58();
  let info: Awaited<ReturnType<typeof readTransferHook>>;
  try {
    info = await readTransferHook(connection, mint);
  } catch (error) {
    const missing = error instanceof Error && /does not exist/.test(error.message);
    return {
      mint: address,
      kind: missing ? 'missing' : 'invalid',
      message: missing ? `No account at ${address} on ${environment.cluster === 'devnet' ? 'devnet' : 'this validator'}.` : 'That address is not a token mint.',
    };
  }
  if (!info.hookProgramId) {
    return { mint: address, kind: 'plain', message: 'A token mint with no Transfer Hook (or none set yet).' };
  }
  const programs = [new PublicKey(environment.cpmmProgramId), new PublicKey(environment.clmmProgramId)];
  const records = programs.map((program) => PublicKey.findProgramAddressSync([Buffer.from('support_mint'), mint.toBytes()], program)[0]);
  const accounts = await connection.getMultipleAccountsInfo(records, 'confirmed');
  return {
    mint: address,
    kind: 'hooked',
    message: `A Token-2022 mint with the Transfer Hook program ${info.hookProgramId.toBase58()}.`,
    hookProgram: info.hookProgramId.toBase58(),
    hookAuthority: info.hookAuthority?.toBase58() ?? null,
    approval: { cpmm: judge(accounts[0], programs[0], mint), clmm: judge(accounts[1], programs[1], mint) },
  };
}

/** The commands that take a token from "has a hook" to "has a pool", with this environment and token filled in. */
export function nextCommands(environment: HookEnvironment, check: TokenCheck): { title: string; command: string }[] {
  const env = environment.cluster === 'devnet' ? 'environments/devnet.json' : 'environments/localnet.json';
  const admin = environment.cluster === 'devnet' ? '.keys/deployer.json' : 'tests/fixtures/localnet/admin.json';
  const commands: { title: string; command: string }[] = [];
  if (check.kind === 'hooked' && check.approval && (check.approval.cpmm !== 'approved' || check.approval.clmm !== 'approved')) {
    commands.push({
      title: 'The pool admin approves the token (only the admin key can; it is the gate that decides whether a pool may hold a hooked token)',
      command: `raydium-hook mint approve --env ${env} --keypair ${admin} --mint ${check.mint}`,
    });
  }
  commands.push({
    title: 'Check the approval at any time (read-only, no key)',
    command: `raydium-hook mint approval --env ${env} --mint ${check.mint}`,
  });
  return commands;
}

/** The command that builds a token and a pool around a hook program of your own. */
export function bringYourHookCommand(environment: HookEnvironment): string {
  const env = environment.cluster === 'devnet' ? 'environments/devnet.json' : 'environments/localnet.json';
  const admin = environment.cluster === 'devnet' ? '.keys/deployer.json' : 'tests/fixtures/localnet/admin.json';
  return `raydium-hook e2e --env ${env} --keypair ${admin} --amm cpmm --hook-dir ./your-hook --keep-state pool.json`;
}
