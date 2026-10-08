// Concurrent hooked swaps on one local validator: does a hook that writes shared state slow
// simultaneous trades more than a hook that only reads its config, and does it matter whether the
// trades hit one pool or several pools of the same hooked mint?
//
//   cargo xtask localnet validator          (leave running)
//   cargo xtask localnet ui-fixture --wallet <any pubkey> --amm cpmm --hook reference \
//       --extra-pools 3 --seed-amount 400000000 --out target/contention/stateless.json
//   cargo xtask localnet ui-fixture --wallet <any pubkey> --amm cpmm --hook fair-launch \
//       --max-buys-per-slot 100000 --extra-pools 3 --seed-amount 400000000 --out target/contention/stateful.json
//   node --experimental-transform-types benches/contention/contention.ts
//
// Four scenarios: the starter (reads its config) and fair-launch (also writes one counter account per
// mint on every buy), each with every buy on ONE pool and with the buys spread over all the pools of
// the mint. On one pool the pool state and its vaults are write-contended in every scenario; spread
// out, the only account the buys can share is the hook's per-mint one. The numbers are from a
// single-node test validator and say nothing about a real cluster's scheduler, leaders or latency
// (see benches/contention/README.md).

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Raydium } from '@raydium-io/raydium-sdk-v2';
import { compileV0, loadEnvironment } from '@raydium-transfer-hook/client';
import {
  TOKEN_2022_PROGRAM_ID,
  createAssociatedTokenAccountIdempotentInstruction,
  createMintToInstruction,
  getAssociatedTokenAddressSync,
} from '@solana/spl-token';
import { Connection, Keypair, PublicKey, SystemProgram, Transaction, VersionedTransaction } from '@solana/web3.js';
import { cpmmAdapter } from '../../apps/fair-launch-ui/src/adapters/cpmm.ts';
import { prepareSwap } from '../../apps/fair-launch-ui/src/lib/swap.ts';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const CONCURRENCY = (process.env.CONTENTION_K ?? '1,4,8,16').split(',').map(Number);
const ROUNDS = Number(process.env.CONTENTION_ROUNDS ?? 5);
const BUY = 1_000_000n; // one token of six decimals
const WALLETS = Math.max(...CONCURRENCY);

interface Fixture {
  pool: string;
  extra_pools?: string[];
  hooked_mint: string;
  quote_mint: string;
}

const environment = loadEnvironment(readFileSync(join(root, 'environments', 'localnet.json'), 'utf8'));
const connection = new Connection(environment.rpcUrl, 'confirmed');
const admin = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync(join(root, 'tests/fixtures/localnet/admin.json'), 'utf8'))));

const load = (name: string): Fixture => JSON.parse(readFileSync(join(root, 'target', 'contention', `${name}.json`), 'utf8'));
const stateless = load('stateless');
const stateful = load('stateful');
// Optional: fair-launch with a per-slot budget (--max-buys-per-slot 3), to see whether the budget is shared
// by the pools of one mint.
const budget = existsSync(join(root, 'target', 'contention', 'budget.json')) ? load('budget') : null;
const poolsOf = (fixture: Fixture): string[] => [fixture.pool, ...(fixture.extra_pools ?? [])];
const scenarios: { name: string; fixture: Fixture; launch: boolean; spread: boolean; minK?: number }[] = [
  { name: 'stateless hook (starter: reads its config)', fixture: stateless, launch: false, spread: false },
  { name: 'stateful hook (fair-launch: writes a counter per buy)', fixture: stateful, launch: true, spread: false },
  { name: 'stateless hook (starter: reads its config)', fixture: stateless, launch: false, spread: true },
  { name: 'stateful hook (fair-launch: writes a counter per buy)', fixture: stateful, launch: true, spread: true },
  ...(budget
    ? [
        { name: 'fair-launch, per-slot budget of 3', fixture: budget, launch: true, spread: false, minK: 8 },
        { name: 'fair-launch, per-slot budget of 3', fixture: budget, launch: true, spread: true, minK: 8 },
      ]
    : []),
].filter((scenario) => !scenario.spread || poolsOf(scenario.fixture).length > 1);

async function send(tx: Transaction, signers: Keypair[]): Promise<void> {
  const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash('confirmed');
  tx.recentBlockhash = blockhash;
  tx.feePayer = admin.publicKey;
  tx.sign(admin, ...signers);
  const signature = await connection.sendRawTransaction(tx.serialize());
  await connection.confirmTransaction({ signature, blockhash, lastValidBlockHeight }, 'confirmed');
}

const median = (values: number[]): number => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)] ?? 0;
const percentile = (values: number[], p: number): number =>
  [...values].sort((a, b) => a - b)[Math.min(values.length - 1, Math.floor(values.length * p))] ?? 0;

interface Outcome {
  ok: boolean;
  ms: number;
  slot: number | null;
  error?: string;
}

type Pool = Awaited<ReturnType<typeof cpmmAdapter.loadPool>>;

/** K buys at once; buyer i trades on pools[i mod pools.length]. */
async function round(
  buyers: { keypair: Keypair }[],
  k: number,
  pools: Pool[],
  launchMint: PublicKey | null
): Promise<Outcome[]> {
  const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash('confirmed');
  const signed: VersionedTransaction[] = [];
  for (const [index, { keypair }] of buyers.slice(0, k).entries()) {
    const pool = pools[index % pools.length]!;
    const quote = cpmmAdapter.quote(pool, false, BUY, 9_900);
    const prepared = await prepareSwap({
      connection,
      environment,
      pool,
      quote,
      inputIsA: false,
      amountIn: BUY,
      payer: keypair.publicKey,
      hooked: launchMint ? { side: 'A', mint: launchMint, kind: 'fair-launch' } : null,
    });
    const tx = compileV0(keypair.publicKey, blockhash, prepared.instructions);
    tx.sign([keypair]);
    signed.push(tx);
  }
  // Fire them all at once and time each from its own send to its confirmation.
  const results = await Promise.all(
    signed.map(async (tx): Promise<Outcome> => {
      const started = performance.now();
      try {
        const signature = await connection.sendRawTransaction(tx.serialize(), { skipPreflight: true });
        const done = await connection.confirmTransaction({ signature, blockhash, lastValidBlockHeight }, 'confirmed');
        const ms = performance.now() - started;
        const status = (await connection.getSignatureStatuses([signature], { searchTransactionHistory: true })).value[0];
        if (done.value.err) return { ok: false, ms, slot: status?.slot ?? null, error: JSON.stringify(done.value.err) };
        return { ok: true, ms, slot: status?.slot ?? null };
      } catch (error) {
        return { ok: false, ms: performance.now() - started, slot: null, error: String(error).slice(0, 120) };
      }
    })
  );
  return results;
}

async function main(): Promise<void> {
  const raydium = await Raydium.load({ connection, cluster: 'mainnet', disableLoadToken: true, disableFeatureCheck: true, owner: undefined } as never);

  // Buyers: fresh wallets, each with SOL and quote tokens; the admin is the mint authority.
  const buyers = Array.from({ length: WALLETS }, () => ({ keypair: Keypair.generate() }));
  for (const { keypair } of buyers) {
    const tx = new Transaction().add(SystemProgram.transfer({ fromPubkey: admin.publicKey, toPubkey: keypair.publicKey, lamports: 200_000_000 }));
    for (const fixture of [stateless, stateful, ...(budget ? [budget] : [])]) {
      const mint = new PublicKey(fixture.quote_mint);
      const account = getAssociatedTokenAddressSync(mint, keypair.publicKey, false, TOKEN_2022_PROGRAM_ID);
      tx.add(createAssociatedTokenAccountIdempotentInstruction(admin.publicKey, account, keypair.publicKey, mint, TOKEN_2022_PROGRAM_ID));
      tx.add(createMintToInstruction(mint, account, admin.publicKey, 100_000_000n, [], TOKEN_2022_PROGRAM_ID));
    }
    await send(tx, []);
  }
  console.log(`prepared ${buyers.length} funded buyers\n`);

  const rows: string[] = [];
  const report: unknown[] = [];
  for (const k of CONCURRENCY) {
    for (const scenario of scenarios) {
      if (scenario.minK && k < scenario.minK) continue;
      const addresses = scenario.spread ? poolsOf(scenario.fixture) : [scenario.fixture.pool];
      const launchMint = scenario.launch ? new PublicKey(scenario.fixture.hooked_mint) : null;
      const times: number[] = [];
      const slots: number[] = [];
      let failed = 0;
      const errors = new Set<string>();
      let perSlotMax = 0;
      for (let r = 0; r < ROUNDS; r += 1) {
        // The pool changes with every trade; reload it so each round quotes the current reserves.
        const fresh = await Promise.all(addresses.map((address) => cpmmAdapter.loadPool({ connection, raydium, environment }, new PublicKey(address))));
        const outcomes = await round(buyers, k, fresh, launchMint);
        const counts = new Map<number, number>();
        for (const outcome of outcomes) {
          if (outcome.ok) {
            times.push(outcome.ms);
            if (outcome.slot !== null) {
              slots.push(outcome.slot);
              counts.set(outcome.slot, (counts.get(outcome.slot) ?? 0) + 1);
            }
          } else {
            failed += 1;
            if (outcome.error) errors.add(outcome.error);
          }
        }
        perSlotMax = Math.max(perSlotMax, ...counts.values(), 0);
      }
      const total = k * ROUNDS;
      const row = `| ${k} | ${scenario.name} | ${addresses.length} | ${total - failed}/${total} | ${median(times).toFixed(0)} | ${percentile(times, 0.95).toFixed(0)} | ${new Set(slots).size} | ${perSlotMax} |`;
      rows.push(row);
      report.push({ k, scenario: scenario.name, pools: addresses.length, ok: total - failed, total, medianMs: median(times), p95Ms: percentile(times, 0.95), slots: new Set(slots).size, maxInOneSlot: perSlotMax, errors: [...errors] });
      console.log(row + (errors.size ? `   errors: ${[...errors].join(' | ')}` : ''));
    }
  }
  console.log('\n| concurrent buys | hook | pools | landed | median ms | p95 ms | slots used | most in one slot |\n|---|---|---|---|---|---|---|---|\n' + rows.join('\n'));
  writeFileSync(join(root, 'target', 'contention', 'result.json'), JSON.stringify(report, null, 2));
}

await main();
