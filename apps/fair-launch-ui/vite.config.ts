/// <reference types="vitest/config" />
import { existsSync } from 'node:fs';
import type { IncomingMessage, ServerResponse } from 'node:http';
import react from '@vitejs/plugin-react';
import { type Plugin, type ProxyOptions, defineConfig, loadEnv } from 'vite';
import { PublicKey } from '@solana/web3.js';
import { connectionFor, faucetFor, fundWallet, readKeypair } from './scripts/faucet-core.mjs';
import { createJobs, poolsFor } from './scripts/pools-core.mjs';

/**
 * `/faucet` for the dev and preview servers: mint a pool's test tokens into a wallet. The mint authority's
 * keypair stays on the server (see `scripts/faucet-core.mjs`); only loopback callers are served, a wallet can ask
 * once every few seconds, and a static build has no such route.
 */
function faucet(env: Record<string, string>): Plugin {
  const lastRequest = new Map<string, number>();
  const send = (res: ServerResponse, status: number, body: unknown) => {
    res.statusCode = status;
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify(body));
  };
  const readBody = (req: IncomingMessage): Promise<string> =>
    new Promise((resolveBody, reject) => {
      let text = '';
      req.on('data', (chunk) => {
        text += chunk;
        if (text.length > 4096) reject(new Error('request too large'));
      });
      req.on('end', () => resolveBody(text));
      req.on('error', reject);
    });
  const available = (cluster: string): boolean => {
    const config = faucetFor(cluster, env);
    return config !== null && existsSync(config.keypairPath);
  };
  const handler = async (req: IncomingMessage, res: ServerResponse) => {
    const remote = req.socket.remoteAddress ?? '';
    if (!['127.0.0.1', '::1', '::ffff:127.0.0.1'].includes(remote)) return send(res, 403, { error: 'the faucet answers this machine only' });
    if (req.method === 'GET') return send(res, 200, { devnet: available('devnet'), localnet: available('localnet') });
    if (req.method !== 'POST') return send(res, 405, { error: 'POST only' });
    try {
      const body = JSON.parse(await readBody(req)) as { cluster?: string; wallet?: string; mints?: string[]; tokens?: number };
      const cluster = body.cluster ?? '';
      const config = faucetFor(cluster, env);
      if (!config || !existsSync(config.keypairPath)) return send(res, 404, { error: `no faucet is set up for ${cluster || 'this cluster'}` });
      if (!body.wallet || !Array.isArray(body.mints) || body.mints.length === 0 || body.mints.length > 4) return send(res, 400, { error: 'send a wallet and one to four mints' });
      new PublicKey(body.wallet);
      body.mints.forEach((mint) => new PublicKey(mint));
      const key = `${cluster}:${body.wallet}`;
      const last = lastRequest.get(key) ?? 0;
      if (Date.now() - last < 5000) return send(res, 429, { error: 'wait a few seconds before asking again' });
      lastRequest.set(key, Date.now());
      const answer = await fundWallet({
        connection: connectionFor(config),
        authority: readKeypair(config.keypairPath),
        wallet: body.wallet,
        mints: body.mints,
        tokens: BigInt(Math.floor(body.tokens ?? 100)),
      });
      return send(res, 200, answer);
    } catch (error) {
      return send(res, 400, { error: error instanceof Error ? error.message : String(error) });
    }
  };
  return {
    name: 'faucet',
    configureServer: (server) => void server.middlewares.use('/faucet', (req, res) => void handler(req, res)),
    configurePreviewServer: (server) => void server.middlewares.use('/faucet', (req, res) => void handler(req, res)),
  };
}

/**
 * `/pools` for the dev and preview servers: create a demo pool (a new hooked token and a real pool of our forked Raydium)
 * by running this repository's own command, and report how it is going. Loopback callers only; see `scripts/pools-core.mjs`.
 */
function pools(env: Record<string, string>): Plugin {
  const jobs = createJobs();
  const send = (res: ServerResponse, status: number, body: unknown) => {
    res.statusCode = status;
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify(body));
  };
  const handler = (req: IncomingMessage, res: ServerResponse) => {
    const remote = req.socket.remoteAddress ?? '';
    if (!['127.0.0.1', '::1', '::ffff:127.0.0.1'].includes(remote)) return send(res, 403, { error: 'this answers this machine only' });
    const path = (req.url ?? '/').split('?')[0];
    if (req.method === 'GET' && (path === '/' || path === '')) return send(res, 200, { devnet: poolsFor('devnet', env) !== null, localnet: poolsFor('localnet', env) !== null });
    if (req.method === 'GET') {
      const job = jobs.view(path.replace(/^\//, ''));
      return job ? send(res, 200, job) : send(res, 404, { error: 'no such pool run' });
    }
    if (req.method !== 'POST') return send(res, 405, { error: 'GET or POST only' });
    let text = '';
    req.on('data', (chunk) => {
      text += chunk;
    });
    req.on('end', () => {
      try {
        const body = JSON.parse(text) as { cluster?: string; hook?: string; amm?: string; wallet?: string };
        if (!body.wallet) return send(res, 400, { error: 'send the wallet that should receive the tokens' });
        new PublicKey(body.wallet);
        const started = jobs.start({ cluster: body.cluster ?? '', hook: body.hook ?? '', amm: body.amm ?? '', wallet: body.wallet }, env);
        return send(res, 'error' in started ? 429 : 202, started);
      } catch (error) {
        return send(res, 400, { error: error instanceof Error ? error.message : String(error) });
      }
    });
  };
  return {
    name: 'pools',
    configureServer: (server) => void server.middlewares.use('/pools', handler),
    configurePreviewServer: (server) => void server.middlewares.use('/pools', handler),
  };
}

export default defineConfig(({ mode }) => {
  // `TRITON_DEVNET_RPC_URL` (no `VITE_` prefix, so Vite never exposes it to the page) lives in the ignored
  // `.env.local`. The dev server forwards `/triton/devnet` to it, token and all, so the browser can read devnet
  // transactions through Triton One without ever holding the URL.
  const env = loadEnv(mode, process.cwd(), '');
  const triton = env.TRITON_DEVNET_RPC_URL ? new URL(env.TRITON_DEVNET_RPC_URL) : null;
  const tritonProxy: Record<string, ProxyOptions> = triton
    ? {
        '/triton/devnet': {
          target: triton.origin,
          changeOrigin: true,
          secure: true,
          rewrite: () => `${triton.pathname}${triton.search}`,
        },
      }
    : {};
  return {
    plugins: [react(), faucet(env), pools(env)],
    define: { global: 'globalThis', __TRITON_DEVNET_PROXY__: JSON.stringify(triton !== null) },
    server: { host: '127.0.0.1', port: 5173, strictPort: true, proxy: tritonProxy },
    preview: { proxy: tritonProxy },
    build: { target: 'es2022', chunkSizeWarningLimit: 2500 },
    test: {
      environment: 'jsdom',
      include: ['test/**/*.test.{ts,tsx}'],
      setupFiles: ['test/setup.ts'],
    },
  };
});
