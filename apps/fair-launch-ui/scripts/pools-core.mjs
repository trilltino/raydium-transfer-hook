// "Create a demo pool": the dev server runs this repository's own pool-creation command and hands the result
// back to the page. It builds a new hooked token and a real pool of our forked Raydium and funds the given wallet,
// exactly as `raydium-hook ui-fixture` does from a terminal.
//
// The pool is made with the key that is the pool admin (our deployer on devnet, the committed fixture admin on a
// local validator): creating a pool for a hooked token needs the admin's per-mint approval, which a browser wallet
// cannot give. So this is a demo and development tool that runs on the machine that has that key; the key never
// reaches the page.

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { repoRoot } from './faucet-core.mjs';

export const HOOKS = ['fair-launch', 'creator-commitment', 'holder-rewards'];
export const AMMS = ['cpmm', 'clmm'];
const COOLDOWN_MS = 60_000;
const out = resolve(repoRoot, 'target', 'ui-pools');

/** Every limit a launch hook is set up with, as raw token units; the window starts when the pool is made. */
const SETTINGS = [
  ['--max-transfer', '1000000000000'],
  ['--window-seconds', '604800'],
  ['--vest-seconds', '604800'],
  ['--locked-total', '80000000'],
  ['--reward-seconds', '3600'],
  ['--reward-amount', '3600000000'],
  ['--max-buy', '100000000'],
  ['--max-wallet', '300000000'],
  ['--max-buys-per-slot', '3'],
  ['--max-priority', '1000'],
  ['--seed-amount', '2000000000'],
  ['--wallet-hooked-amount', '100000000'],
  ['--wallet-quote-amount', '1000000000'],
];

/** What can create a pool for a cluster: devnet needs the admin keypair (`FAUCET_KEYPAIR`), a local validator nothing. */
export function poolsFor(cluster, env) {
  if (cluster === 'localnet') return { cluster };
  if (cluster === 'devnet' && env.FAUCET_KEYPAIR) {
    const keypair = resolve(repoRoot, env.FAUCET_KEYPAIR);
    const feeReceiver = resolve(repoRoot, env.FEE_RECEIVER_KEYPAIR || '.keys/cpmm-fee-receiver.json');
    if (existsSync(keypair) && existsSync(feeReceiver)) return { cluster, keypair, feeReceiver };
  }
  return null;
}

export function createJobs() {
  const jobs = new Map();
  const lastStart = new Map();
  let running = false;

  /** Start a pool-creation run; returns `{ id }` or `{ error }`. Only one runs at a time, and each cluster has a cooldown. */
  function start({ cluster, hook, amm, wallet }, env) {
    const config = poolsFor(cluster, env);
    if (!config) return { error: `no pool can be created for ${cluster || 'this cluster'} here` };
    if (!HOOKS.includes(hook) || !AMMS.includes(amm)) return { error: 'choose a hook and an AMM from the lists' };
    if (running) return { error: 'a pool is already being created' };
    const wait = (lastStart.get(cluster) ?? 0) + COOLDOWN_MS - Date.now();
    if (wait > 0) return { error: `wait ${Math.ceil(wait / 1000)} seconds before creating another pool` };

    mkdirSync(out, { recursive: true });
    const id = randomBytes(6).toString('hex');
    const file = resolve(out, `${id}.json`);
    const common = ['--wallet', wallet, '--amm', amm, '--hook', hook, '--out', file];
    const args =
      cluster === 'localnet'
        ? ['xtask', 'localnet', 'ui-fixture', ...common]
        : [
            'run', '--quiet', '-p', 'raydium-hook-cli', '--',
            'ui-fixture', '--env', 'environments/devnet.json', '--keypair', config.keypair, '--fee-receiver-keypair', config.feeReceiver,
            ...common, ...SETTINGS.flat(), '--wallet-lamports', '0',
          ];
    const job = { id, status: 'running', log: [], cluster, hook, amm, startedAt: Date.now() };
    jobs.set(id, job);
    running = true;
    lastStart.set(cluster, Date.now());
    const child = spawn('cargo', args, { cwd: repoRoot, env: { ...process.env, RUST_LOG: 'off' }, windowsHide: true });
    const take = (chunk) => {
      for (const line of String(chunk).split(/\r?\n/)) if (line.trim()) job.log.push(line.replace(/\x1b\[[0-9;]*m/g, '').slice(0, 300));
      if (job.log.length > 200) job.log.splice(0, job.log.length - 200);
    };
    child.stdout.on('data', take);
    child.stderr.on('data', take);
    child.on('error', (error) => {
      job.status = 'failed';
      job.error = `could not start cargo: ${error.message}`;
      running = false;
    });
    child.on('close', (code) => {
      running = false;
      if (code === 0 && existsSync(file)) {
        const made = JSON.parse(readFileSync(file, 'utf8'));
        Object.assign(job, { status: 'done', pool: made.pool, hookedMint: made.hooked_mint, quoteMint: made.quote_mint, hookProgram: made.hook_program });
      } else {
        job.status = 'failed';
        job.error = job.log.at(-1) ?? `the pool command exited with ${code}`;
      }
    });
    return { id };
  }

  /** What the page sees of a run: its state and the last lines it printed. */
  function view(id) {
    const job = jobs.get(id);
    if (!job) return null;
    const { status, error, pool, hookedMint, quoteMint, hookProgram, hook, amm, cluster } = job;
    return { status, error, pool, hookedMint, quoteMint, hookProgram, hook, amm, cluster, log: job.log.slice(-6) };
  }

  return { start, view };
}
