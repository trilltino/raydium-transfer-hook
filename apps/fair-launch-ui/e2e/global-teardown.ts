import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const pidFile = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'target', 'ui-e2e', 'validator.pid');

/** Stop the validator only if global setup started it. */
export default async function globalTeardown(): Promise<void> {
  if (!existsSync(pidFile)) return;
  const pid = readFileSync(pidFile, 'utf8').trim();
  if (!pid) return;
  try {
    if (process.platform === 'win32') execFileSync('taskkill', ['/PID', pid, '/T', '/F'], { stdio: 'ignore' });
    else execFileSync('pkill', ['-f', 'solana-test-validator'], { stdio: 'ignore' });
  } catch {
    // already gone
  }
  try {
    if (process.platform === 'win32') execFileSync('taskkill', ['/IM', 'solana-test-validator.exe', '/F'], { stdio: 'ignore' });
  } catch {
    // none running
  }
}
