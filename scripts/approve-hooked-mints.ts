/**
 * Approve hooked mints for pool creation on the Raydium CPMM / CLMM forks, or check whether they are
 * approved. No Raydium SDK is needed: the instruction is an 8-byte discriminator and four accounts.
 *
 * Raydium's CPMM and CLMM admit a Token-2022 mint with a TransferHook extension to a new pool only if a
 * `SupportMintAssociated` record exists for it. The programs accept exactly two signers for
 * `create_support_mint_associated`: their compile-time admin and one fixed owner key. So whoever deploys the
 * forks with their own keys approves the mints; nobody else can.
 *
 * This is the same job as `raydium-hook mint approve` / `mint approval` (the Rust CLI), for people who
 * would rather run a script. Run with --help for usage.
 */
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { TOKEN_2022_PROGRAM_ID, getTransferHook, unpackMint } from '@solana/spl-token';
import {
  Connection,
  Keypair,
  PACKET_DATA_SIZE,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  sendAndConfirmTransaction,
  type AccountInfo,
} from '@solana/web3.js';

export type Amm = 'cpmm' | 'clmm';

export const RECORD_SEED = Buffer.from('support_mint');

/** Anchor's discriminator: the first eight bytes of sha256("<namespace>:<name>"). */
export const anchorDiscriminator = (namespace: 'global' | 'account', name: string): Buffer =>
  createHash('sha256').update(`${namespace}:${name}`).digest().subarray(0, 8);

export const recordAddress = (program: PublicKey, mint: PublicKey): PublicKey =>
  PublicKey.findProgramAddressSync([RECORD_SEED, mint.toBuffer()], program)[0];

/** The program's `create_support_mint_associated`: owner (signer, writable), mint, record (writable), system. */
export function createSupportMintInstruction(
  program: PublicKey,
  admin: PublicKey,
  mint: PublicKey
): TransactionInstruction {
  return new TransactionInstruction({
    programId: program,
    keys: [
      { pubkey: admin, isSigner: true, isWritable: true },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: recordAddress(program, mint), isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: anchorDiscriminator('global', 'create_support_mint_associated'),
  });
}

export type RecordState =
  | { state: 'approved' }
  | { state: 'not-approved' }
  | { state: 'invalid'; reason: string };

/**
 * Judge the account found at a record address. The program owns the address, so a correct owner,
 * discriminator and stored mint is what `create_support_mint_associated` leaves behind.
 */
export function judgeRecord(
  account: AccountInfo<Buffer> | null,
  program: PublicKey,
  mint: PublicKey
): RecordState {
  if (account === null) return { state: 'not-approved' };
  if (account.executable || !account.owner.equals(program)) {
    return { state: 'invalid', reason: `owned by ${account.owner.toBase58()}, not by the program` };
  }
  // 8-byte discriminator, 1-byte bump, 32-byte mint, then padding.
  if (account.data.length < 41 || !account.data.subarray(0, 8).equals(anchorDiscriminator('account', 'SupportMintAssociated'))) {
    return { state: 'invalid', reason: 'not a SupportMintAssociated account' };
  }
  if (!account.data.subarray(9, 41).equals(mint.toBuffer())) {
    return { state: 'invalid', reason: 'the record names a different mint' };
  }
  return { state: 'approved' };
}

/** One mint address per line. Blank lines and `#` comments (whole-line or after the address) are ignored. */
export function parseMintsFile(text: string): PublicKey[] {
  const mints: PublicKey[] = [];
  text.split(/\r?\n/).forEach((raw, index) => {
    const line = raw.split('#')[0].trim();
    if (!line) return;
    try {
      mints.push(new PublicKey(line));
    } catch {
      throw new Error(`line ${index + 1}: \`${line}\` is not a public key`);
    }
  });
  return mints;
}

/** Remove anything that looks like a URL (an RPC URL can carry an API key) from an error message. */
export const redact = (message: string): string =>
  message
    .split(/\s+/)
    .map((word) => (word.includes('://') ? '[URL redacted]' : word))
    .join(' ');

// ---------------------------------------------------------------------------------------------
// Packing into transactions
// ---------------------------------------------------------------------------------------------

const buildTransaction = (
  instructions: TransactionInstruction[],
  payer: PublicKey,
  blockhash: string
): Transaction => new Transaction({ feePayer: payer, recentBlockhash: blockhash }).add(...instructions);

/** Serialised size: the compact signature count, every 64-byte signature, then the message. */
export function transactionSize(transaction: Transaction): number {
  const message = transaction.compileMessage();
  return 1 + 64 * message.header.numRequiredSignatures + message.serialize().length;
}

/** Group instructions, in order, into the fewest transactions that each fit a packet. */
export function packInstructions(
  instructions: TransactionInstruction[],
  payer: PublicKey,
  blockhash: string
): Transaction[] {
  const tooBig = () => new Error('one approval instruction does not fit in a transaction');
  const batches: Transaction[] = [];
  let current: TransactionInstruction[] = [];
  for (const instruction of instructions) {
    if (transactionSize(buildTransaction([...current, instruction], payer, blockhash)) <= PACKET_DATA_SIZE) {
      current.push(instruction);
      continue;
    }
    if (!current.length) throw tooBig();
    batches.push(buildTransaction(current, payer, blockhash));
    current = [instruction];
    if (transactionSize(buildTransaction(current, payer, blockhash)) > PACKET_DATA_SIZE) throw tooBig();
  }
  if (current.length) batches.push(buildTransaction(current, payer, blockhash));
  return batches;
}

// ---------------------------------------------------------------------------------------------
// The environment file (the same files the Rust CLI reads)
// ---------------------------------------------------------------------------------------------

export interface Environment {
  name: string;
  cluster: string;
  rpcUrl: string;
  admin: PublicKey;
  programs: Record<Amm, PublicKey>;
}

export function parseEnvironment(json: string): Environment {
  const raw = JSON.parse(json);
  const need = (value: unknown, what: string): string => {
    if (typeof value !== 'string' || !value) throw new Error(`the environment file has no ${what}`);
    return value;
  };
  const key = (value: unknown, what: string): PublicKey => {
    const text = need(value, what);
    try {
      return new PublicKey(text);
    } catch {
      throw new Error(`the environment file's ${what} is not a public key`);
    }
  };
  return {
    name: String(raw.name ?? ''),
    cluster: String(raw.cluster ?? ''),
    rpcUrl: need(raw.rpc_url, 'rpc_url'),
    admin: key(raw.admin, 'admin'),
    programs: { cpmm: key(raw.programs?.cpmm, 'programs.cpmm'), clmm: key(raw.programs?.clmm, 'programs.clmm') },
  };
}

export const loadKeypair = (path: string): Keypair =>
  Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync(path, 'utf8'))));

export const parseAmms = (text: string): Amm[] => {
  if (text === 'cpmm' || text === 'clmm') return [text];
  if (text === 'all') return ['cpmm', 'clmm'];
  throw new Error(`unknown AMM \`${text}\`: cpmm, clmm or all`);
};

// ---------------------------------------------------------------------------------------------
// Checks on the mint, and on the record
// ---------------------------------------------------------------------------------------------

export type MintCheck =
  | { kind: 'approvable'; note: string }
  | { kind: 'not-needed'; reason: string }
  | { kind: 'blocked'; reason: string };

/**
 * Whether a mint can and needs to be approved. A mint whose hook is not set yet is approvable, because
 * the usual order is to approve the mint, create the pool, then attach the hook. (The Rust command also
 * checks the hook's validation list and reports a problem as a note; this script does not.)
 */
export async function checkMint(
  connection: Pick<Connection, 'getAccountInfo'>,
  mint: PublicKey
): Promise<MintCheck> {
  const account = await connection.getAccountInfo(mint, 'confirmed');
  if (account === null) return { kind: 'blocked', reason: 'the mint account does not exist' };
  if (!account.owner.equals(TOKEN_2022_PROGRAM_ID)) return { kind: 'blocked', reason: 'not a Token-2022 mint' };
  let hook;
  try {
    hook = getTransferHook(unpackMint(mint, account, TOKEN_2022_PROGRAM_ID));
  } catch {
    return { kind: 'blocked', reason: 'the account is not a readable Token-2022 mint' };
  }
  if (hook === null) {
    return { kind: 'not-needed', reason: 'the mint has no TransferHook extension, so no approval is needed' };
  }
  if (hook.programId.equals(PublicKey.default)) {
    return { kind: 'approvable', note: 'the hook is not set yet; attach it after the pool exists' };
  }
  const program = await connection.getAccountInfo(hook.programId, 'confirmed');
  if (program === null) return { kind: 'blocked', reason: `hook program ${hook.programId.toBase58()} does not exist` };
  if (!program.executable) return { kind: 'blocked', reason: `hook program ${hook.programId.toBase58()} is not executable` };
  return { kind: 'approvable', note: '' };
}

export async function recordState(
  connection: Pick<Connection, 'getAccountInfo'>,
  program: PublicKey,
  mint: PublicKey
): Promise<RecordState> {
  return judgeRecord(await connection.getAccountInfo(recordAddress(program, mint), 'confirmed'), program, mint);
}

// ---------------------------------------------------------------------------------------------
// approve
// ---------------------------------------------------------------------------------------------

export type Outcome =
  | { kind: 'already-approved' }
  | { kind: 'approved'; signature: string }
  | { kind: 'would-approve' }
  | { kind: 'not-needed'; reason: string }
  | { kind: 'blocked'; reason: string }
  | { kind: 'failed'; reason: string };

export interface Row {
  amm: Amm;
  mint: PublicKey;
  outcome: Outcome;
  note: string;
}

export const isProblem = (outcome: Outcome): boolean => outcome.kind === 'blocked' || outcome.kind === 'failed';

export function describeOutcome(outcome: Outcome): string {
  switch (outcome.kind) {
    case 'already-approved':
      return 'already approved';
    case 'approved':
      return `approved  ${outcome.signature}`;
    case 'would-approve':
      return 'would approve (dry run, simulation passed)';
    case 'not-needed':
      return `not needed: ${outcome.reason}`;
    case 'blocked':
      return `NOT approved: ${outcome.reason}`;
    case 'failed':
      return `FAILED: ${outcome.reason}`;
  }
}

/**
 * Approve `mints` on `amms`. The signer must be the program admin the environment records. A mint that is
 * blocked or fails never stops the others; every transaction is simulated before it is sent.
 */
export async function approve(
  connection: Connection,
  env: Environment,
  admin: Keypair,
  amms: Amm[],
  mints: PublicKey[],
  dryRun: boolean
): Promise<Row[]> {
  if (!admin.publicKey.equals(env.admin)) {
    throw new Error(
      `this key (${admin.publicKey.toBase58()}) cannot approve mints: the program admin recorded in the environment is ` +
        `${env.admin.toBase58()}. Only the admin (or the one fixed owner key built into the program) can sign the approval.`
    );
  }
  const unique = [...new Map(mints.map((mint) => [mint.toBase58(), mint])).values()];
  const checks = new Map<string, MintCheck>();
  for (const mint of unique) checks.set(mint.toBase58(), await checkMint(connection, mint));

  const rows: Row[] = [];
  for (const amm of amms) {
    const program = env.programs[amm];
    const pending: { mint: PublicKey; note: string }[] = [];
    for (const mint of unique) {
      const check = checks.get(mint.toBase58())!;
      if (check.kind === 'blocked') {
        rows.push({ amm, mint, outcome: { kind: 'blocked', reason: check.reason }, note: '' });
      } else if (check.kind === 'not-needed') {
        rows.push({ amm, mint, outcome: { kind: 'not-needed', reason: check.reason }, note: '' });
      } else {
        const state = await recordState(connection, program, mint);
        if (state.state === 'approved') {
          rows.push({ amm, mint, outcome: { kind: 'already-approved' }, note: check.note });
        } else if (state.state === 'invalid') {
          rows.push({
            amm,
            mint,
            outcome: {
              kind: 'blocked',
              reason: `an account already sits at the record address but is not a valid record: ${state.reason}`,
            },
            note: '',
          });
        } else {
          pending.push({ mint, note: check.note });
        }
      }
    }
    if (!pending.length) continue;

    const { blockhash } = await connection.getLatestBlockhash('confirmed');
    const batches = packInstructions(
      pending.map(({ mint }) => createSupportMintInstruction(program, admin.publicKey, mint)),
      admin.publicKey,
      blockhash
    );
    let offset = 0;
    for (const batch of batches) {
      const covered = pending.slice(offset, offset + batch.instructions.length);
      offset += batch.instructions.length;
      let outcome: Outcome;
      try {
        const simulation = await connection.simulateTransaction(batch, [admin]);
        if (simulation.value.err) {
          outcome = { kind: 'failed', reason: redact(`the simulation failed: ${JSON.stringify(simulation.value.err)}`) };
        } else if (dryRun) {
          outcome = { kind: 'would-approve' };
        } else {
          const signature = await sendAndConfirmTransaction(connection, batch, [admin], { commitment: 'confirmed' });
          outcome = { kind: 'approved', signature };
        }
      } catch (error) {
        outcome = { kind: 'failed', reason: redact(error instanceof Error ? error.message : 'unknown error') };
      }
      for (const { mint, note } of covered) rows.push({ amm, mint, outcome, note });
    }
  }
  return rows;
}

// ---------------------------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------------------------

const USAGE = `Usage:
  approve-hooked-mints.ts approve --env FILE --keypair ADMIN.json (--mint MINT ... | --mints-file FILE)
                          [--amm cpmm|clmm|all] [--dry-run]
  approve-hooked-mints.ts status  --env FILE (--mint MINT ... | --mints-file FILE) [--amm cpmm|clmm|all]

approve  Approve hooked mints so a pool can be created with them. The keypair must be the admin the
         environment records. Mints already approved are skipped; each transaction is simulated first;
         --dry-run stops there.
status   Read-only, no keypair: is each mint approved on each AMM?

--env is an environment file such as environments/localnet.json. --mint can be repeated; the mints
file has one address per line (# comments allowed). Exit status is 1 if any approval did not go through.`;

function explorer(env: Environment, signature: string): string {
  if (env.cluster === 'devnet') return `https://explorer.solana.com/tx/${signature}?cluster=devnet`;
  if (env.cluster === 'mainnet-beta') return `https://explorer.solana.com/tx/${signature}`;
  return signature;
}

function collectMints(values: { mint?: string[]; 'mints-file'?: string }): PublicKey[] {
  const mints: PublicKey[] = [];
  for (const text of values.mint ?? []) {
    try {
      mints.push(new PublicKey(text));
    } catch {
      throw new Error(`--mint ${text}: not a public key`);
    }
  }
  if (values['mints-file']) {
    const file = values['mints-file'];
    try {
      mints.push(...parseMintsFile(readFileSync(file, 'utf8')));
    } catch (error) {
      throw new Error(`${file}: ${error instanceof Error ? error.message : error}`);
    }
  }
  const unique = [...new Map(mints.map((mint) => [mint.toBase58(), mint])).values()];
  if (!unique.length) throw new Error('give at least one mint: --mint MINT (repeatable) or --mints-file FILE');
  return unique;
}

async function main(argv: string[]): Promise<number> {
  const [command, ...rest] = argv;
  if (!command || command === '--help' || command === '-h' || command === 'help') {
    console.log(USAGE);
    return 0;
  }
  if (command !== 'approve' && command !== 'status') throw new Error(`unknown command \`${command}\`\n\n${USAGE}`);
  const { values } = parseArgs({
    args: rest,
    strict: true,
    options: {
      env: { type: 'string' },
      keypair: { type: 'string' },
      mint: { type: 'string', multiple: true },
      'mints-file': { type: 'string' },
      amm: { type: 'string', default: 'all' },
      'dry-run': { type: 'boolean', default: false },
    },
  });
  if (!values.env) throw new Error(`missing --env\n\n${USAGE}`);
  const env = parseEnvironment(readFileSync(values.env, 'utf8'));
  const mints = collectMints(values);
  const amms = parseAmms(values.amm ?? 'all');
  const connection = new Connection(env.rpcUrl, 'confirmed');

  if (command === 'status') {
    for (const mint of mints) {
      const check = await checkMint(connection, mint);
      const what =
        check.kind === 'approvable'
          ? check.note
            ? `approvable (${check.note})`
            : 'ready to approve'
          : check.kind === 'not-needed'
            ? `no approval needed (${check.reason})`
            : `cannot be approved (${check.reason})`;
      console.log(`${mint.toBase58()}  ${what}`);
      for (const amm of amms) {
        const state = await recordState(connection, env.programs[amm], mint);
        const text =
          state.state === 'approved'
            ? 'approved: a pool can be created'
            : state.state === 'not-approved'
              ? 'NOT approved: ask the operator to run `approve`'
              : `INVALID record: ${state.reason}`;
        console.log(`  ${amm.padEnd(5)} ${text}`);
      }
    }
    return 0;
  }

  if (!values.keypair) throw new Error(`missing --keypair\n\n${USAGE}`);
  const admin = loadKeypair(values.keypair);
  const dryRun = values['dry-run'] ?? false;
  console.log(
    `approving ${mints.length} mint(s) on ${amms.join(' and ')} as ${admin.publicKey.toBase58()}${
      dryRun ? ' (dry run: nothing is sent)' : ''
    }\n`
  );
  const rows = await approve(connection, env, admin, amms, mints, dryRun);
  let problems = 0;
  for (const row of rows) {
    console.log(`${row.amm.padEnd(5)} ${row.mint.toBase58()}  ${describeOutcome(row.outcome)}`);
    if (row.outcome.kind === 'approved') console.log(`      ${explorer(env, row.outcome.signature)}`);
    if (row.note) console.log(`      note: ${row.note}`);
    if (isProblem(row.outcome)) problems += 1;
  }
  if (problems) {
    console.error(`\n${problems} approval(s) did not go through; see above`);
    return 1;
  }
  console.log(
    dryRun
      ? '\ndry run complete: the simulations passed and nothing was sent.'
      : '\ndone. A pool can now be created with these mints; check any time with `status`.'
  );
  return 0;
}

const isMain = (url: string): boolean => !!process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === url;

if (isMain(import.meta.url)) {
  main(process.argv.slice(2)).then(
    (code) => {
      process.exitCode = code;
    },
    (error) => {
      console.error(redact(error instanceof Error ? error.message : 'Unknown error'));
      process.exitCode = 1;
    }
  );
}
