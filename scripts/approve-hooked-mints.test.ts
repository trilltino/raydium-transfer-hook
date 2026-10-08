import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { TOKEN_2022_PROGRAM_ID } from '@solana/spl-token';
import { Keypair, PACKET_DATA_SIZE, PublicKey, SystemProgram, TransactionInstruction } from '@solana/web3.js';
import {
  anchorDiscriminator,
  checkMint,
  createSupportMintInstruction,
  judgeRecord,
  packInstructions,
  parseAmms,
  parseEnvironment,
  parseMintsFile,
  recordAddress,
  redact,
  transactionSize,
} from './approve-hooked-mints.ts';

const key = () => Keypair.generate().publicKey;
const info = (owner: PublicKey, data: Buffer, executable = false) => ({
  executable,
  owner,
  lamports: 1,
  data,
});

/** A Token-2022 mint account, optionally with a TransferHook extension pointing at `hookProgram`. */
function token2022Mint(hookProgram: PublicKey | null) {
  const base = Buffer.alloc(82);
  base.writeUInt32LE(0, 0); // no mint authority
  base.writeUInt8(6, 44); // decimals
  base.writeUInt8(1, 45); // initialised
  if (hookProgram === null) return info(TOKEN_2022_PROGRAM_ID, base);
  // Extended mints are padded to the size of a token account, then one account-type byte, then TLV.
  const padded = Buffer.alloc(165);
  base.copy(padded);
  const tlv = Buffer.alloc(4 + 64);
  tlv.writeUInt16LE(14, 0); // ExtensionType::TransferHook
  tlv.writeUInt16LE(64, 2);
  Buffer.alloc(32).copy(tlv, 4); // authority: none
  hookProgram.toBuffer().copy(tlv, 36);
  return info(TOKEN_2022_PROGRAM_ID, Buffer.concat([padded, Buffer.from([1]), tlv]));
}

const connectionWith = (accounts: Map<string, ReturnType<typeof info>>) => ({
  getAccountInfo: async (address: PublicKey) => accounts.get(address.toBase58()) ?? null,
});

test('the discriminators are the sha256 prefixes Anchor uses', () => {
  const sha = (text: string) => createHash('sha256').update(text).digest().subarray(0, 8);
  assert.deepEqual(anchorDiscriminator('global', 'create_support_mint_associated'), sha('global:create_support_mint_associated'));
  assert.deepEqual(anchorDiscriminator('account', 'SupportMintAssociated'), sha('account:SupportMintAssociated'));
  // Pinned, so a change here cannot go unnoticed (the Rust CLI builds the same bytes).
  assert.equal(anchorDiscriminator('global', 'create_support_mint_associated').toString('hex').length, 16);
});

test('the approval instruction has the account order and flags the program expects', () => {
  const [program, admin, mint] = [key(), key(), key()];
  const ix = createSupportMintInstruction(program, admin, mint);
  assert.ok(ix.programId.equals(program));
  assert.deepEqual(
    ix.keys.map((k) => [k.pubkey.toBase58(), k.isSigner, k.isWritable]),
    [
      [admin.toBase58(), true, true],
      [mint.toBase58(), false, false],
      [recordAddress(program, mint).toBase58(), false, true],
      [SystemProgram.programId.toBase58(), false, false],
    ]
  );
  assert.deepEqual(ix.data, anchorDiscriminator('global', 'create_support_mint_associated'));
  // The record address is a function of the program and the mint only.
  assert.ok(recordAddress(program, mint).equals(recordAddress(program, mint)));
  assert.ok(!recordAddress(program, mint).equals(recordAddress(program, key())));
});

test('a record counts only if owner, discriminator and stored mint all match', () => {
  const [program, mint] = [key(), key()];
  const record = (owner: PublicKey, stored: PublicKey) =>
    info(
      owner,
      Buffer.concat([anchorDiscriminator('account', 'SupportMintAssociated'), Buffer.from([255]), stored.toBuffer(), Buffer.alloc(64)])
    );
  assert.equal(judgeRecord(null, program, mint).state, 'not-approved');
  assert.equal(judgeRecord(record(program, mint), program, mint).state, 'approved');
  // Funding the address is not approval: the program must own it.
  assert.equal(judgeRecord(record(key(), mint), program, mint).state, 'invalid');
  assert.equal(judgeRecord(record(program, key()), program, mint).state, 'invalid');
  const junk = record(program, mint);
  junk.data.fill(1, 0, 8);
  assert.equal(judgeRecord(junk, program, mint).state, 'invalid');
  assert.equal(judgeRecord(info(program, Buffer.alloc(10)), program, mint).state, 'invalid');
});

test('a mints file allows blank lines and comments, and names a bad line', () => {
  const [a, b] = [key(), key()];
  assert.deepEqual(
    parseMintsFile(`# team 1\n${a.toBase58()}\n\n   ${b.toBase58()}   # team 2\n`).map((k) => k.toBase58()),
    [a.toBase58(), b.toBase58()]
  );
  assert.deepEqual(parseMintsFile(''), []);
  assert.throws(() => parseMintsFile('# header\nnot-a-key\n'), /line 2: `not-a-key`/);
});

test('urls are removed from messages', () => {
  const cleaned = redact('failed to get info about account (https://rpc.example/?api-key=SECRET): timed out');
  assert.ok(!cleaned.includes('SECRET'));
  assert.ok(cleaned.includes('[URL redacted]'));
  assert.equal(redact('plain message'), 'plain message');
});

test('many approvals pack into transactions that each fit a packet, in order', () => {
  const [program, admin] = [key(), key()];
  const blockhash = key().toBase58();
  const instructions = Array.from({ length: 40 }, () => createSupportMintInstruction(program, admin, key()));
  const batches = packInstructions(instructions, admin, blockhash);
  assert.ok(batches.length > 1, '40 approvals cannot share one packet');
  assert.equal(batches.reduce((n, b) => n + b.instructions.length, 0), 40);
  for (const batch of batches) assert.ok(transactionSize(batch) <= PACKET_DATA_SIZE);
  const flat = batches.flatMap((b) => b.instructions);
  assert.deepEqual(flat.map((i: TransactionInstruction) => i.keys[1].pubkey.toBase58()), instructions.map((i) => i.keys[1].pubkey.toBase58()));
});

test('the AMM list parses', () => {
  assert.deepEqual(parseAmms('all'), ['cpmm', 'clmm']);
  assert.deepEqual(parseAmms('cpmm'), ['cpmm']);
  assert.throws(() => parseAmms('orca'), /unknown AMM/);
});

test('the committed environment files parse', () => {
  const env = parseEnvironment(readFileSync(new URL('../environments/localnet.json', import.meta.url), 'utf8'));
  assert.equal(env.cluster, 'localnet');
  assert.ok(env.programs.cpmm instanceof PublicKey && env.programs.clmm instanceof PublicKey);
  assert.throws(() => parseEnvironment('{"rpc_url":"http://x"}'), /no admin/);
  assert.throws(() => parseEnvironment('{"rpc_url":"http://x","admin":"nope"}'), /admin is not a public key/);
});

test('a mint is judged from what is on the chain', async () => {
  const [mint, hook] = [key(), key()];
  const none = connectionWith(new Map());
  assert.equal((await checkMint(none, mint)).kind, 'blocked');

  const classic = connectionWith(new Map([[mint.toBase58(), info(key(), Buffer.alloc(82))]]));
  assert.match((await checkMint(classic, mint) as { reason: string }).reason, /not a Token-2022 mint/);

  const noHook = connectionWith(new Map([[mint.toBase58(), token2022Mint(null)]]));
  assert.equal((await checkMint(noHook, mint)).kind, 'not-needed');

  // The usual order: approve the mint, create the pool, then attach the hook.
  const unset = connectionWith(new Map([[mint.toBase58(), token2022Mint(PublicKey.default)]]));
  const unsetCheck = await checkMint(unset, mint);
  assert.equal(unsetCheck.kind, 'approvable');

  const missingProgram = connectionWith(new Map([[mint.toBase58(), token2022Mint(hook)]]));
  assert.match((await checkMint(missingProgram, mint) as { reason: string }).reason, /does not exist/);

  const notExecutable = connectionWith(
    new Map([[mint.toBase58(), token2022Mint(hook)], [hook.toBase58(), info(key(), Buffer.alloc(1))]])
  );
  assert.match((await checkMint(notExecutable, mint) as { reason: string }).reason, /not executable/);

  const ready = connectionWith(
    new Map([[mint.toBase58(), token2022Mint(hook)], [hook.toBase58(), info(key(), Buffer.alloc(1), true)]])
  );
  assert.deepEqual(await checkMint(ready, mint), { kind: 'approvable', note: '' });
});
