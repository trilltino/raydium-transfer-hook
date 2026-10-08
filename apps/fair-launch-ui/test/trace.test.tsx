import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TransactionTrace } from '../src/components/TransactionTrace.tsx';
import { solscanUrl } from '../src/lib/solscan.ts';
import { type TraceSource, traceSourceFor, traceTransaction } from '../src/lib/trace-client.ts';
import { base58Decode, buildTrace, invocationsFromLogs } from '../src/lib/trace.ts';
import swapResult from './data/clmm-swap.json';

// A real answer from our Triton One devnet endpoint (`getTransaction`, `jsonParsed`) for a hooked CLMM
// `swap_v3` on the integration devnet. Nothing in it is secret: it is public chain data.
const SIGNATURE = '2ngY8AGjBws95QxsVvNGy5RBGRkJSbVP9EKuYpXeprWDPqjTuCtAjqApmXjVCPBWypwCpDrGiBQfjnb9hGTDp9DM';
/** A fetch that answers every JSON-RPC call with `next()`, echoing the request id the way a real node does. */
const rpcFetch = (next: () => unknown, onUrl?: (url: string) => void): typeof fetch =>
  (async (url: unknown, init?: RequestInit) => {
    onUrl?.(String(url));
    const { id } = JSON.parse(String(init?.body));
    return new Response(JSON.stringify({ jsonrpc: '2.0', id, result: next() }), { headers: { 'content-type': 'application/json' } });
  }) as unknown as typeof fetch;

const devnet: HookEnvironment = {
  name: 'integration-devnet',
  cluster: 'devnet',
  rpcUrl: 'https://api.devnet.solana.com',
  cpmmProgramId: '7tRJH4mmEfNGGLf9E8qEvo3oSjq2b75DhSggb1Wz45fJ',
  clmmProgramId: '3dNJapViueBArDN3HbWKcEf2u6hfZQ3oDyh3GmUXJ8oD',
  fairLaunchProgramId: '7xyk1AQg7xCaQucPgWs13hmAs4dmD214raNgEZhLPQSu',
};
const localnet: HookEnvironment = { ...devnet, name: 'localnet', cluster: 'localnet', rpcUrl: 'http://127.0.0.1:8899' };

/** A source that parses the recorded answer exactly as web3.js parses an RPC reply. */
function recorded(): TraceSource {
  return traceSourceFor(devnet, { triton: true, origin: 'http://127.0.0.1:5173', fetch: rpcFetch(() => swapResult) });
}

afterEach(() => vi.unstubAllGlobals());

describe('Solscan links', () => {
  it('names the devnet cluster, and opens a local validator through Solscan custom RPC', () => {
    expect(solscanUrl(devnet, 'tx', 'SIG')).toBe('https://solscan.io/tx/SIG?cluster=devnet');
    expect(solscanUrl(devnet, 'account', 'KEY')).toBe('https://solscan.io/account/KEY?cluster=devnet');
    expect(solscanUrl(devnet, 'token', 'MINT')).toBe('https://solscan.io/token/MINT?cluster=devnet');
    expect(solscanUrl(localnet, 'tx', 'SIG')).toBe('https://solscan.io/tx/SIG?cluster=custom&customUrl=http%3A%2F%2F127.0.0.1%3A8899');
  });
});

describe('reading the logs', () => {
  it('nests each program under the one that called it and attributes compute and logs to the right one', () => {
    const nodes = invocationsFromLogs([
      'Program A invoke [1]',
      'Program log: outer',
      'Program B invoke [2]',
      'Program log: inner',
      'Program B consumed 40 of 900 compute units',
      'Program B success',
      'Program log: outer again',
      'Program A consumed 100 of 1000 compute units',
      'Program A success',
      'Program C invoke [1]',
      'Program C failed: custom program error: 0x1',
    ]);
    expect(nodes.map((n) => [n.programId, n.depth, n.computeUnits, n.failed])).toEqual([
      ['A', 1, 100, false],
      ['B', 2, 40, false],
      ['C', 1, null, true],
    ]);
    expect(nodes[0].logs).toEqual(['Program log: outer', 'Program log: outer again']);
    expect(nodes[1].logs).toEqual(['Program log: inner']);
  });

  it('decodes base58 instruction data', () => {
    expect([...base58Decode('3Bxs3zzLZLuLQEYX')]).toEqual([2, 0, 0, 0, 0, 202, 154, 59, 0, 0, 0, 0]);
    expect([...base58Decode('115T')]).toEqual([0, 0, 1, 2]);
  });
});

describe('building the trace from a real answer', () => {
  it('keeps the runtime order and nesting: swap, then each Token-2022 transfer with the hook inside it', async () => {
    const transaction = (await recorded().fetchTransaction(SIGNATURE))!;
    const trace = buildTrace(transaction, devnet, 'Triton One (devnet)');
    expect(trace.steps.map((step) => [step.depth, step.programName, step.instruction])).toEqual([
      [1, 'Compute Budget', 'set_compute_unit_limit'],
      [1, 'Raydium CLMM (hook-aware fork)', 'swap_v3'],
      [2, 'Token-2022', 'transfer_checked'],
      [3, expect.stringMatching(/^Transfer Hook /), 'Execute (Transfer Hook)'],
      [2, 'Token-2022', 'transfer_checked'],
    ]);
    expect(trace.steps.map((step) => step.number)).toEqual([1, 2, 3, 4, 5]);
  });

  it('carries each step’s compute, accounts, and the transfer’s decoded fields', async () => {
    const trace = buildTrace((await recorded().fetchTransaction(SIGNATURE))!, devnet, 'x');
    expect(trace.steps[1].computeUnits).toBe(109176);
    expect(trace.steps[1].accounts).toHaveLength(18);
    expect(trace.steps[2].details.find((d) => d.name === 'amount')?.value).toBe('10');
    expect(trace.steps[2].details.find((d) => d.name === 'mint')?.value).toBe('37XdnjoqrCbWFae3JQfA5t8gABMjGZkrpmBUuG3GkCAo');
    expect(trace.steps[1].logs.some((line) => line.includes('Instruction: SwapV3'))).toBe(true);
  });

  it('counts how often the hook ran and reports the transaction totals', async () => {
    const trace = buildTrace((await recorded().fetchTransaction(SIGNATURE))!, devnet, 'Triton One (devnet)');
    expect(trace.hookRuns).toEqual([{ programId: 'Cz3Ge1ENd1yZAbtXA88dYaxA8x4ZkSxxdHmSj7nxQ11X', programName: expect.any(String), count: 1 }]);
    expect(trace).toMatchObject({ success: true, slot: 508780616, computeUnits: 109326, computeLimit: 1_400_000, feeLamports: 5000, source: 'Triton One (devnet)' });
  });
});

describe('where the page reads from', () => {
  it('uses the Triton proxy for devnet when the dev server has one, and the environment’s own RPC otherwise', async () => {
    const urls: string[] = [];
    const spy = rpcFetch(() => null, (url) => urls.push(url));
    const viaTriton = traceSourceFor(devnet, { triton: true, origin: 'http://127.0.0.1:5173', fetch: spy });
    const publicRpc = traceSourceFor(devnet, { triton: false, fetch: spy });
    const local = traceSourceFor(localnet, { triton: true, fetch: spy });
    expect([viaTriton.label, publicRpc.label, local.label]).toEqual(['Triton One (devnet)', 'public devnet RPC', 'local validator']);
    await viaTriton.fetchTransaction(SIGNATURE);
    await publicRpc.fetchTransaction(SIGNATURE);
    await local.fetchTransaction(SIGNATURE);
    expect(urls).toEqual(['http://127.0.0.1:5173/triton/devnet', 'https://api.devnet.solana.com', 'http://127.0.0.1:8899']);
  });
});

describe('asking for a trace', () => {
  it('retries while the RPC has not indexed a fresh transaction, then succeeds', async () => {
    const replies: unknown[] = [null, null, swapResult];
    const source = traceSourceFor(devnet, { triton: true, origin: 'http://x', fetch: rpcFetch(() => replies.shift()) });
    const waits: number[] = [];
    const attempts: number[] = [];
    const result = await traceTransaction({
      environment: devnet,
      signature: SIGNATURE,
      source,
      wait: async (ms) => void waits.push(ms),
      onAttempt: (n) => attempts.push(n),
    });
    expect(result.status).toBe('ready');
    expect(attempts).toEqual([1, 2, 3]);
    expect(waits).toEqual([2000, 2000]);
  });

  it('reports a signature that never shows up, after the attempts it was allowed', async () => {
    const source = traceSourceFor(devnet, { triton: true, origin: 'http://x', fetch: rpcFetch(() => null) });
    const result = await traceTransaction({ environment: devnet, signature: SIGNATURE, source, wait: async () => undefined, attempts: 2 });
    expect(result).toMatchObject({ status: 'not-observed', message: expect.stringContaining('Triton One (devnet)') });
  });

  it('turns an RPC failure into a plain note naming where it tried to read', async () => {
    const source = traceSourceFor(devnet, {
      triton: true,
      origin: 'http://x',
      fetch: (async () => {
        throw new TypeError('fetch failed');
      }) as unknown as typeof fetch,
    });
    const result = await traceTransaction({ environment: devnet, signature: SIGNATURE, source, wait: async () => undefined, attempts: 1 });
    expect(result).toMatchObject({ status: 'unavailable', message: expect.stringContaining('Triton One (devnet)') });
  });
});

describe('the trace card', () => {
  it('lists every step with Solscan links for the transaction, each program and each account', async () => {
    render(<TransactionTrace environment={devnet} signature={SIGNATURE} source={recorded()} />);
    const summary = await screen.findByTestId('trace-summary');
    expect(summary.textContent).toContain('Landed in slot 508780616');
    expect(summary.textContent).toContain('109,326 of 1,400,000 CU');
    expect(screen.getByTestId('trace-hooks').textContent).toContain('ran 1 time');
    const steps = screen.getAllByTestId('trace-step');
    expect(steps).toHaveLength(5);
    expect(within(steps[1]).getByText('swap_v3')).toBeTruthy();
    expect(within(steps[3]).getByText('Execute (Transfer Hook)')).toBeTruthy();
    expect(steps[3].getAttribute('data-depth')).toBe('3');
    expect(screen.getByTestId('solscan-tx').getAttribute('href')).toBe(`https://solscan.io/tx/${SIGNATURE}?cluster=devnet`);
    const program = within(steps[1]).getByRole('link', { name: 'program ↗' });
    expect(program.getAttribute('href')).toBe('https://solscan.io/account/3dNJapViueBArDN3HbWKcEf2u6hfZQ3oDyh3GmUXJ8oD?cluster=devnet');
    // the accounts inside a step are links too
    const account = within(steps[1])
      .getAllByRole('link')
      .find((link) => link.getAttribute('href')?.includes('/account/9aH1L4UXQ23MX69VHqRzuCWbEmGMM5nzTTgCqpdUovn5'));
    expect(account).toBeTruthy();
  });
});
