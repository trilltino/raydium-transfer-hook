import { type ChildProcess, execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { newWallet, reachable, RPC_URL } from './rpc.ts';

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
  let validator: ChildProcess | null = null;
  if (!(await reachable())) {
    validator = spawn('cargo', ['xtask', 'localnet', 'validator'], { cwd: repo, shell: true, stdio: 'ignore' });
    writeFileSync(join(outDir, 'validator.pid'), String(validator.pid));
    await waitForRpc(180_000);
  } else {
    writeFileSync(join(outDir, 'validator.pid'), '');
  }

  // E2E_REUSE=1 keeps the pools and wallet of the previous run (they live as long as the validator does).
  if (process.env.E2E_REUSE === '1' && existsSync(join(outDir, 'fixture-clmm.json'))) return;

  const wallet = newWallet();
  writeFileSync(join(outDir, 'wallet.json'), JSON.stringify(wallet.secret));
  for (const amm of ['cpmm', 'clmm']) {
    execFileSync(
      'cargo',
      ['xtask', 'localnet', 'ui-fixture', '--wallet', wallet.address, '--amm', amm, '--out', join(outDir, `fixture-${amm}.json`)],
      { cwd: repo, stdio: 'inherit', shell: true }
    );
  }
}
