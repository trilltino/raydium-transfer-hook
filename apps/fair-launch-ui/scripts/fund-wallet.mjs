// Put test tokens of a pool into a wallet:
//
//   npm --workspace apps/fair-launch-ui run fund -- <WALLET> --pool <POOL> [--tokens 100] [--cluster devnet|localnet]
//   npm --workspace apps/fair-launch-ui run fund -- <WALLET> --mint <MINT> [--mint <MINT>] ...
//
// It mints from the key that built the pool, which only a Node process may hold: on devnet put the path of that
// keypair in apps/fair-launch-ui/.env.local as FAUCET_KEYPAIR (see .env.example); a local validator uses the
// committed fixture admin and needs nothing. The same code answers the page's "Get test tokens" button.

import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { loadEnv } from 'vite';
import { DEFAULT_TOKENS, connectionFor, faucetFor, fundWallet, readKeypair, readPoolMints, repoRoot } from './faucet-core.mjs';

const args = process.argv.slice(2);
const flag = (name) => args.flatMap((arg, i) => (arg === `--${name}` ? [args[i + 1]] : []));
const wallet = args.find((arg) => !arg.startsWith('--') && !flag('pool').includes(arg) && !flag('mint').includes(arg) && !flag('tokens').includes(arg) && !flag('cluster').includes(arg));
const cluster = flag('cluster')[0] ?? 'devnet';
const tokens = BigInt(flag('tokens')[0] ?? DEFAULT_TOKENS);
const pool = flag('pool')[0];
const mints = flag('mint');

if (!wallet || (!pool && mints.length === 0)) {
  console.error('usage: fund-wallet.mjs <WALLET> (--pool <POOL> | --mint <MINT> ...) [--tokens 100] [--cluster devnet|localnet]');
  process.exit(2);
}

const env = { ...loadEnv('development', process.cwd(), ''), ...process.env };
const config = faucetFor(cluster, env);
if (!config) {
  console.error(`no faucet for ${cluster}: set FAUCET_KEYPAIR in apps/fair-launch-ui/.env.local to the keypair file that built the pool`);
  process.exit(1);
}
if (!existsSync(config.keypairPath)) {
  console.error(`faucet keypair not found: ${config.keypairPath}`);
  process.exit(1);
}
const environmentFile = JSON.parse(readFileSync(resolve(repoRoot, 'environments', cluster === 'devnet' ? 'devnet.json' : 'localnet.json'), 'utf8'));
const programs = { cpmm: environmentFile.programs.cpmm, clmm: environmentFile.programs.clmm };

const connection = connectionFor(config);
const authority = readKeypair(config.keypairPath);
const targets = mints.length > 0 ? mints : (await readPoolMints(connection, pool, programs)).map((key) => key.toBase58());
const { signature, results } = await fundWallet({ connection, authority, wallet, mints: targets, tokens });
for (const result of results) {
  console.log(
    result.status === 'minted'
      ? `minted ${tokens} of ${result.mint} into ${result.account}`
      : `skipped ${result.mint}: ${result.reason}`
  );
}
if (signature) console.log(`transaction ${signature}\nhttps://solscan.io/tx/${signature}${cluster === 'devnet' ? '?cluster=devnet' : ''}`);
else process.exitCode = 1;
