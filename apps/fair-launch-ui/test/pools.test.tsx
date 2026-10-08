import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { type Connection, Keypair, PublicKey } from '@solana/web3.js';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const hook = vi.hoisted(() => ({ read: vi.fn() }));
vi.mock('@raydium-transfer-hook/client', async (original) => ({ ...(await original<typeof import('@raydium-transfer-hook/client')>()), readTransferHook: hook.read }));

import { BringYourOwnToken } from '../src/components/BringYourOwnToken.tsx';
import { CreateDemoPool } from '../src/components/CreateDemoPool.tsx';
import { followPool, poolCreationAvailable, startPool } from '../src/lib/pools-client.ts';
import { bringYourHookCommand, checkToken, nextCommands } from '../src/lib/token-check.ts';

const devnet: HookEnvironment = {
  name: 'integration-devnet',
  cluster: 'devnet',
  rpcUrl: 'https://api.devnet.solana.com',
  cpmmProgramId: '7tRJH4mmEfNGGLf9E8qEvo3oSjq2b75DhSggb1Wz45fJ',
  clmmProgramId: '3dNJapViueBArDN3HbWKcEf2u6hfZQ3oDyh3GmUXJ8oD',
  fairLaunchProgramId: '7xyk1AQg7xCaQucPgWs13hmAs4dmD214raNgEZhLPQSu',
};
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
const asFetch = (fn: (url: unknown, init?: RequestInit) => Promise<Response>) => fn as unknown as typeof fetch;
const MINT = Keypair.fromSeed(Buffer.alloc(32, 5)).publicKey;
const HOOK = Keypair.fromSeed(Buffer.alloc(32, 6)).publicKey;

/** The approval record the pool admin's `mint approve` leaves behind: owned by the AMM, discriminator, bump, mint. */
function record(program: string, mint: PublicKey, wrongMint = false) {
  const data = Buffer.alloc(8 + 1 + 32 + 8);
  Buffer.from('8628b74f0c70a235', 'hex').copy(data, 0);
  (wrongMint ? Keypair.generate().publicKey : mint).toBuffer().copy(data, 9);
  return { owner: new PublicKey(program), executable: false, data: Uint8Array.from(data) };
}
const chain = (records: unknown[]) => ({ getMultipleAccountsInfo: vi.fn(async () => records) }) as unknown as Connection;

beforeEach(() => hook.read.mockReset());
afterEach(() => vi.unstubAllGlobals());

describe('the pool endpoint client', () => {
  it('asks whether this cluster can create pools, and posts the choice with the cluster', async () => {
    expect(await poolCreationAvailable(devnet, asFetch(async () => json({ devnet: true, localnet: false })))).toBe(true);
    expect(await poolCreationAvailable(devnet, asFetch(async () => json({ devnet: false })))).toBe(false);
    expect(
      await poolCreationAvailable(
        devnet,
        asFetch(async () => {
          throw new TypeError('no server');
        })
      )
    ).toBe(false);
    const seen: RequestInit[] = [];
    const id = await startPool(
      devnet,
      { hook: 'fair-launch', amm: 'clmm', wallet: 'W' },
      asFetch(async (_url, init) => {
        seen.push(init!);
        return json({ id: 'abc' }, 202);
      })
    );
    expect(id).toBe('abc');
    expect(JSON.parse(String(seen[0].body))).toEqual({ cluster: 'devnet', hook: 'fair-launch', amm: 'clmm', wallet: 'W' });
    await expect(startPool(devnet, { hook: 'fair-launch', amm: 'cpmm', wallet: 'W' }, asFetch(async () => json({ error: 'wait 40 seconds before creating another pool' }, 429)))).rejects.toThrow('wait 40 seconds');
  });

  it('follows a run until it is done, reporting each state', async () => {
    const states = [
      { status: 'running', log: ['a'] },
      { status: 'running', log: ['a', 'b'] },
      { status: 'done', pool: 'POOL', log: [] },
    ];
    const updates: string[] = [];
    const final = await followPool('abc', (run) => updates.push(run.status), {
      fetchFn: asFetch(async () => json(states.shift())),
      wait: async () => undefined,
    });
    expect(final.pool).toBe('POOL');
    expect(updates).toEqual(['running', 'running', 'done']);
  });
});

describe('Create a demo pool', () => {
  it('says what it needs when the dev server cannot create pools, and offers no button', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => json({ devnet: false })));
    render(<CreateDemoPool environment={devnet} wallet={null} onOpen={() => undefined} />);
    fireEvent.click(screen.getByRole('button', { name: 'Create a demo pool' }));
    expect((await screen.findByTestId('create-pool-unavailable')).textContent).toContain('FAUCET_KEYPAIR');
    expect(screen.queryByRole('button', { name: 'Create pool' })).toBeNull();
  });

  it('creates the pool for the connected wallet with the chosen hook and AMM, shows progress, and opens it', async () => {
    let runs = 0;
    let posted: Record<string, string> = {};
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: RequestInit) => {
        if (init?.method === 'POST') {
          posted = JSON.parse(String(init.body));
          return json({ id: 'run1' }, 202);
        }
        if (String(url) === '/pools') return json({ devnet: true });
        runs += 1;
        return json(runs < 2 ? { status: 'running', log: ['creating the hooked mint'] } : { status: 'done', pool: 'NEWPOOL', log: [] });
      })
    );
    const onOpen = vi.fn();
    render(<CreateDemoPool environment={devnet} wallet="MYWALLET" onOpen={onOpen} />);
    fireEvent.click(screen.getByRole('button', { name: 'Create a demo pool' }));
    const [hook_, amm] = await screen.findAllByRole('combobox');
    fireEvent.change(hook_, { target: { value: 'holder-rewards' } });
    fireEvent.change(amm, { target: { value: 'clmm' } });
    fireEvent.click(await screen.findByRole('button', { name: 'Create pool' }));
    expect((await screen.findByTestId('create-pool-progress')).textContent).toContain('creating the hooked mint');
    await waitFor(() => expect(onOpen).toHaveBeenCalledWith('NEWPOOL'), { timeout: 10_000 });
    expect(posted).toEqual({ cluster: 'devnet', hook: 'holder-rewards', amm: 'clmm', wallet: 'MYWALLET' });
  });

  it('shows the reason when the run fails or is refused', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown, init?: RequestInit) =>
        init?.method === 'POST' ? json({ error: 'a pool is already being created' }, 429) : String(url) === '/pools' ? json({ devnet: true }) : json({})
      )
    );
    render(<CreateDemoPool environment={devnet} wallet={null} onOpen={() => undefined} />);
    fireEvent.click(screen.getByRole('button', { name: 'Create a demo pool' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Create pool' }));
    expect((await screen.findByTestId('create-pool-error')).textContent).toBe('a pool is already being created');
  });
});

describe('checking a token before there is a pool', () => {
  it('reports a hooked mint and whether the pool admin approved it on each AMM', async () => {
    hook.read.mockResolvedValue({ mint: MINT, tokenProgram: PublicKey.default, hookProgramId: HOOK, hookAuthority: null, decimals: 6 });
    const check = await checkToken(chain([record(devnet.cpmmProgramId, MINT), null]), devnet, MINT.toBase58());
    expect(check).toMatchObject({ kind: 'hooked', hookProgram: HOOK.toBase58(), hookAuthority: null, approval: { cpmm: 'approved', clmm: 'not-approved' } });
  });

  it('calls a record that names another mint, or belongs to another program, invalid', async () => {
    hook.read.mockResolvedValue({ mint: MINT, tokenProgram: PublicKey.default, hookProgramId: HOOK, hookAuthority: HOOK, decimals: 6 });
    const check = await checkToken(chain([record(devnet.cpmmProgramId, MINT, true), record(devnet.cpmmProgramId, MINT)]), devnet, MINT.toBase58());
    expect(check.approval).toEqual({ cpmm: 'invalid', clmm: 'invalid' });
  });

  it('says a plain mint has no hook, a missing one is missing, and a non-address is not an address', async () => {
    hook.read.mockResolvedValueOnce({ mint: MINT, tokenProgram: PublicKey.default, hookProgramId: null, hookAuthority: null, decimals: 6 });
    expect((await checkToken(chain([]), devnet, MINT.toBase58())).kind).toBe('plain');
    hook.read.mockRejectedValueOnce(new Error(`the mint ${MINT.toBase58()} does not exist`));
    expect((await checkToken(chain([]), devnet, MINT.toBase58())).kind).toBe('missing');
    expect((await checkToken(chain([]), devnet, 'nope')).kind).toBe('invalid');
  });

  it('prints the approve command only while a mint is not approved on both AMMs, with the environment filled in', () => {
    const base = { mint: MINT.toBase58(), kind: 'hooked' as const, message: '', approval: { cpmm: 'approved' as const, clmm: 'not-approved' as const } };
    expect(nextCommands(devnet, base).map((entry) => entry.command)).toEqual([
      `raydium-hook mint approve --env environments/devnet.json --keypair .keys/deployer.json --mint ${MINT.toBase58()}`,
      `raydium-hook mint approval --env environments/devnet.json --mint ${MINT.toBase58()}`,
    ]);
    expect(nextCommands(devnet, { ...base, approval: { cpmm: 'approved', clmm: 'approved' } }).map((entry) => entry.command)).toEqual([
      `raydium-hook mint approval --env environments/devnet.json --mint ${MINT.toBase58()}`,
    ]);
    expect(bringYourHookCommand(devnet)).toContain('--hook-dir ./your-hook --keep-state pool.json');
  });
});

describe('Bring your own token', () => {
  it('checks a pasted mint and shows the hook, each AMM’s approval and what to run next', async () => {
    hook.read.mockResolvedValue({ mint: MINT, tokenProgram: PublicKey.default, hookProgramId: HOOK, hookAuthority: null, decimals: 6 });
    render(<BringYourOwnToken environment={devnet} connection={chain([record(devnet.cpmmProgramId, MINT), null])} />);
    fireEvent.click(screen.getByRole('button', { name: 'Bring your own token' }));
    expect(screen.getAllByTestId('command')[0].textContent).toContain('--hook-dir ./your-hook');
    fireEvent.change(screen.getByPlaceholderText('Token mint address'), { target: { value: MINT.toBase58() } });
    fireEvent.click(screen.getByRole('button', { name: 'Check token' }));
    const result = await screen.findByTestId('token-check');
    expect(result.getAttribute('data-kind')).toBe('hooked');
    expect(result.textContent).toContain('given up (the hook cannot be changed)');
    const approval = screen.getByTestId('token-approval');
    expect([...approval.querySelectorAll('li')].map((li) => li.getAttribute('data-status'))).toEqual(['approved', 'not-approved']);
    expect(screen.getAllByTestId('command').some((c) => c.textContent?.includes('mint approve'))).toBe(true);
  });
});
