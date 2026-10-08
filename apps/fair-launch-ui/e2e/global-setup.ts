import { type ChildProcess, execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { DEVNET, newWallet, reachable, RPC_URL } from './rpc.ts';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const outDir = join(repo, 'target', 'ui-e2e');

async function waitForRpc(timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await reachable()) return;
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  throw new Error(`no validator answered at ${RPC_URL} within ${timeoutMs / 1000}s`);
}

/**
 * Start the local validator (unless one is already running), give a throwaway wallet a Fair Launch pool
 * and funds, and write what the tests need under target/ui-e2e/. Nothing here is a real key.
 */
export default async function globalSetup(): Promise<void> {
  mkdirSync(outDir, { recursive: true });
  if (DEVNET) return setUpDevnet();
  let validator: ChildProcess | null = null;
  if (!(await reachable())) {
    validator = spawn('cargo', ['xtask', 'localnet', 'validator'], { cwd: repo, shell: true, stdio: 'ignore' });
    writeFileSync(join(outDir, 'validator.pid'), String(validator.pid));
    await waitForRpc(180_000);
  } else {
    writeFileSync(join(outDir, 'validator.pid'), '');
  }

  // E2E_REUSE=1 keeps the pools and wallet of the previous run (they live as long as the validator does).
  if (process.env.E2E_REUSE === '1' && existsSync(join(outDir, 'fixture-holder-rewards-cpmm.json'))) return;

  const wallet = newWallet();
  writeFileSync(join(outDir, 'wallet.json'), JSON.stringify(wallet.secret));
  // Which pools the browser tests use: Fair Launch on both AMMs, the other two example hooks on CPMM.
  const pools = [
    ['fair-launch', 'cpmm'],
    ['fair-launch', 'clmm'],
    ['creator-commitment', 'cpmm'],
    ['holder-rewards', 'cpmm'],
  ];
  for (const [hook, amm] of pools) {
    execFileSync(
      'cargo',
      ['xtask', 'localnet', 'ui-fixture', '--wallet', wallet.address, '--amm', amm, '--hook', hook, '--out', join(outDir, `fixture-${hook}-${amm}.json`)],
      { cwd: repo, stdio: 'inherit', shell: true }
    );
  }
}

/**
 * Against our integration devnet: a pool and a funded throwaway wallet made with the deployer key from
 * `.keys/` (the deployer is the integration build's admin, which pool creation needs). Needs those keys
 * and a few tenths of a SOL; nothing here is used by the deterministic CI run.
 */
function setUpDevnet(): void {
  const wallet = newWallet();
  writeFileSync(join(outDir, 'wallet.json'), JSON.stringify(wallet.secret));
  writeFileSync(join(outDir, 'validator.pid'), '');
  const amms = (process.env.E2E_AMMS ?? 'cpmm').split(',');
  for (const amm of amms) {
    execFileSync(
      'cargo',
      [
        'run', '-q', '-p', 'raydium-hook-cli', '--', 'ui-fixture',
        '--env', 'environments/devnet.json',
        '--keypair', '.keys/deployer.json',
        '--fee-receiver-keypair', '.keys/cpmm-fee-receiver.json',
        '--wallet', wallet.address, '--amm', amm,
        '--out', join(outDir, `fixture-fair-launch-${amm}.json`),
        '--window-seconds', '3600', '--max-buy', '100000000', '--max-wallet', '300000000',
        '--max-buys-per-slot', '3', '--max-priority', '1000',
        '--seed-amount', '2000000000', '--wallet-hooked-amount', '100000000',
        '--wallet-quote-amount', '1000000000', '--wallet-lamports', '300000000',
      ],
      { cwd: repo, stdio: 'inherit', shell: true }
    );
  }
}
