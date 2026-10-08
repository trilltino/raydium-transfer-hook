import { getExtraAccountMetaAddress } from '@solana/spl-token';
import { describe, expect, it } from 'vitest';
import { HookClientError, type TransferLeg, readTransferHook, resolveTransferHookLeg } from '../src/index.ts';
import { MemoryConnection, fresh, key } from './chain.ts';

const hook = key(0x42);
const mint = key(0x11);

function leg(overrides: Partial<TransferLeg> = {}): TransferLeg {
  return { role: 'input', mint, source: key(1), destination: key(2), authority: key(3), amount: 500n, ...overrides };
}

async function refusal(promise: Promise<unknown>): Promise<HookClientError> {
  try {
    await promise;
  } catch (error) {
    expect(error).toBeInstanceOf(HookClientError);
    return error as HookClientError;
  }
  throw new Error('expected the client to refuse');
}

describe('readTransferHook', () => {
  it('reads the hook program and authority of a Token-2022 mint', async () => {
    const chain = new MemoryConnection().addHookedMint(mint, hook, []);
    const info = await readTransferHook(chain.asConnection(), mint);
    expect(info.hookProgramId?.toBase58()).toBe(hook.toBase58());
    expect(info.hookAuthority?.toBase58()).toBe(key(0xa0).toBase58());
    expect(info.decimals).toBe(6);
  });

  it('reports no hook for a classic mint', async () => {
    const chain = new MemoryConnection().addClassicMint(mint);
    expect((await readTransferHook(chain.asConnection(), mint)).hookProgramId).toBeNull();
  });

  it('refuses a missing mint and an account that is not a mint', async () => {
    const chain = new MemoryConnection().set(mint, key(0x77), Buffer.alloc(82));
    expect((await refusal(readTransferHook(chain.asConnection(), fresh()))).kind).toBe('mint-missing');
    expect((await refusal(readTransferHook(chain.asConnection(), mint))).kind).toBe('unsupported-token-program');
  });
});

describe('resolveTransferHookLeg', () => {
  it('gives an empty slice for a mint with no hook', async () => {
    const chain = new MemoryConnection().addClassicMint(mint);
    const resolved = await resolveTransferHookLeg(chain.asConnection(), leg());
    expect(resolved.slice).toEqual([]);
    expect(resolved.hookProgram).toBeNull();
  });

  it('resolves extras, then the hook program, then the validation list', async () => {
    const extra = key(0x51);
    const chain = new MemoryConnection().addHookedMint(mint, hook, [{ pubkey: extra }]);
    const resolved = await resolveTransferHookLeg(chain.asConnection(), leg());
    expect(resolved.slice.map((meta) => meta.pubkey.toBase58())).toEqual([
      extra.toBase58(),
      hook.toBase58(),
      getExtraAccountMetaAddress(mint, hook).toBase58(),
    ]);
    expect(resolved.slice.map((meta) => [meta.isSigner, meta.isWritable])).toEqual([
      [false, false],
      [false, false],
      [false, false],
    ]);
  });

  it('keeps the order the list declares and does not deduplicate', async () => {
    const [a, b] = [key(0x61), key(0x62)];
    const chain = new MemoryConnection().addHookedMint(mint, hook, [{ pubkey: b }, { pubkey: a }, { pubkey: b }]);
    const resolved = await resolveTransferHookLeg(chain.asConnection(), leg());
    expect(resolved.slice.map((meta) => meta.pubkey.toBase58()).slice(0, 3)).toEqual([b, a, b].map((k) => k.toBase58()));
  });

  it('refuses a writable extra unless the caller named it', async () => {
    const state = key(0x52);
    const chain = new MemoryConnection().addHookedMint(mint, hook, [{ pubkey: state, isWritable: true }]);
    expect((await refusal(resolveTransferHookLeg(chain.asConnection(), leg()))).kind).toBe('unexpected-writable');
    const allowed = await resolveTransferHookLeg(chain.asConnection(), leg(), { allowWritable: [state] });
    expect(allowed.slice[0].isWritable).toBe(true);
  });

  it('refuses a signer extra even if it is named writable', async () => {
    const chain = new MemoryConnection().addHookedMint(mint, hook, [{ pubkey: key(0x53), isSigner: true }]);
    const error = await refusal(resolveTransferHookLeg(chain.asConnection(), leg(), { allowWritable: [key(0x53)] }));
    expect(error.kind).toBe('unexpected-signer');
  });

  it('refuses a mint whose hook is not the expected program', async () => {
    const chain = new MemoryConnection().addHookedMint(mint, hook, []);
    const error = await refusal(resolveTransferHookLeg(chain.asConnection(), leg(), { expectedHookProgram: key(0x99) }));
    expect(error.kind).toBe('hook-program-mismatch');
    const unhooked = new MemoryConnection().addClassicMint(mint);
    expect((await refusal(resolveTransferHookLeg(unhooked.asConnection(), leg(), { expectedHookProgram: hook }))).kind).toBe(
      'hook-program-mismatch'
    );
  });

  it('refuses a hook program that is missing or not executable', async () => {
    const missing = new MemoryConnection().addHookedMint(mint, hook, []);
    // overwrite the hook program with a non-executable account
    missing.set(hook, key(0x77), Buffer.alloc(8), false);
    expect((await refusal(resolveTransferHookLeg(missing.asConnection(), leg()))).kind).toBe('hook-program-invalid');
  });

  it('refuses a missing validation list and one the hook does not own', async () => {
    const wrongOwner = new MemoryConnection().addHookedMint(mint, hook, []);
    wrongOwner.set(getExtraAccountMetaAddress(mint, hook), key(0x78), Buffer.alloc(16));
    expect((await refusal(resolveTransferHookLeg(wrongOwner.asConnection(), leg()))).kind).toBe('validation-list-owner');

    const noList = new MemoryConnection().addHookedMint(mint, hook, []);
    noList.remove(getExtraAccountMetaAddress(mint, hook));
    expect((await refusal(resolveTransferHookLeg(noList.asConnection(), leg()))).kind).toBe('validation-list-missing');
  });
});
