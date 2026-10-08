import { HookClientError, type HookFailure, MAX_VENUES } from '@raydium-transfer-hook/client';
import { PublicKey } from '@solana/web3.js';
import { describe, expect, it } from 'vitest';
import { formatAmount, parseAmount, shortKey, toInputText } from '../src/lib/amounts.ts';
import { checkPool, parsePoolParam } from '../src/lib/pool.ts';
import { presentClientError, presentSimulationFailure } from '../src/lib/present.ts';
import { quoteBaseInput } from '../src/lib/quote.ts';
import { swapButtonState } from '../src/components/SwapCard.tsx';
import { adapterForKind, adapterForOwner, cpmmAdapter } from '../src/adapters/index.ts';
import { key, launchConfig, poolView } from './fixtures.ts';

describe('amounts', () => {
  it('parses plain decimals into base units', () => {
    expect(parseAmount('1.25', 6)).toBe(1_250_000n);
    expect(parseAmount('0.000001', 6)).toBe(1n);
    expect(parseAmount('1,000', 0)).toBe(1000n);
    expect(parseAmount('.5', 2)).toBe(50n);
  });

  it('rejects junk, too many decimals and amounts over a u64', () => {
    for (const bad of ['', '.', 'abc', '1.2.3', '-1', '1e5', '0.0000001']) expect(parseAmount(bad, 6)).toBeNull();
    expect(parseAmount('18446744073709551616', 0)).toBeNull();
  });

  it('formats with separators and trims zeros', () => {
    expect(formatAmount(1_234_567_890n, 6)).toBe('1,234.56789');
    expect(formatAmount(5n, 0)).toBe('5');
    expect(toInputText(1_500_000n, 6)).toBe('1.5');
    expect(shortKey('11111111111111111111111111111111')).toBe('1111…1111');
  });
});

describe('quote', () => {
  const economics = {
    reserveA: 1_000_000_000n,
    reserveB: 1_000_000_000n,
    tradeFeeRate: 2_500n, // 0.25% of 1e6
    creatorFeeRate: 0n,
    protocolFeeRate: 120_000n,
    fundFeeRate: 40_000n,
    feeOn: 0,
  };

  it('matches the constant-product formula after the trade fee', () => {
    const quote = quoteBaseInput(economics, 1_000_000n, true, 50);
    const fee = (1_000_000n * 2_500n + 999_999n) / 1_000_000n; // fee is rounded up
    const afterFee = 1_000_000n - fee;
    const expected = (afterFee * 1_000_000_000n) / (1_000_000_000n + afterFee);
    expect(quote.amountOut).toBe(expected);
    expect(quote.tradeFee).toBe(fee);
    expect(quote.minimumOut).toBe((expected * 9_950n) / 10_000n);
  });

  it('is symmetric when the pool is balanced and reports price impact', () => {
    const small = quoteBaseInput(economics, 1_000n, true, 50);
    const large = quoteBaseInput(economics, 100_000_000n, true, 50);
    expect(large.priceImpactBps).toBeGreaterThan(small.priceImpactBps);
    expect(quoteBaseInput(economics, 1_000_000n, false, 50).amountOut).toBe(quoteBaseInput(economics, 1_000_000n, true, 50).amountOut);
  });
});

describe('pool selection', () => {
  const program = new PublicKey(Buffer.alloc(32, 7));

  it('parses the pool parameter', () => {
    expect(parsePoolParam(null)).toBeNull();
    expect(parsePoolParam('')).toBeNull();
    expect(parsePoolParam('not a key')).toEqual({ error: '“not a key” is not a valid pool address.' });
    expect(parsePoolParam(program.toBase58())).toEqual(program);
  });

  it('fails closed when the pool belongs to another program', () => {
    const result = checkPool(poolView(), new PublicKey(Buffer.alloc(32, 9)), null);
    expect(result.ok).toBe(false);
  });

  it('accepts the pool of the expected program', () => {
    expect(checkPool(poolView(), poolView().programId, null).ok).toBe(true);
  });

  it('refuses a pool with swaps disabled', () => {
    const pool = poolView({ status: 4 });
    expect(checkPool(pool, pool.programId, null)).toEqual({ ok: false, reason: 'Swaps are disabled on this pool.' });
  });

  it('refuses a launch whose venues do not include the pool vault', () => {
    const pool = poolView();
    const config = launchConfig({ mint: pool.tokenA.mint, venues: [new PublicKey(Buffer.alloc(32, 99))] });
    const result = checkPool(pool, pool.programId, { config, hookedSide: 'A' });
    expect(result.ok).toBe(false);
    expect(MAX_VENUES).toBe(4);
  });

  it('refuses a launch configured for a different mint', () => {
    const pool = poolView();
    const config = launchConfig({ mint: new PublicKey(Buffer.alloc(32, 98)), venues: [pool.tokenA.vault] });
    expect(checkPool(pool, pool.programId, { config, hookedSide: 'A' }).ok).toBe(false);
  });

  it('accepts a launch whose venue is the pool vault', () => {
    const pool = poolView();
    const config = launchConfig({ mint: pool.tokenA.mint, venues: [pool.tokenA.vault] });
    expect(checkPool(pool, pool.programId, { config, hookedSide: 'A' }).ok).toBe(true);
  });
});

describe('failure presentation', () => {
  const fairLaunch = 'FairLaunch1111111111111111111111111111111111';
  const hookFailure = (code: number, programId = fairLaunch): HookFailure => ({
    kind: 'hook',
    programId,
    code,
    codeHex: `0x${code.toString(16).toUpperCase()}`,
  });

  it.each([
    [0xb003, 'larger than the launch max-buy'],
    [0xb004, 'would exceed the configured launch limit'],
    [0xb005, 'no more buys in this slot'],
    [0xb006, 'priority fee above the launch limit'],
  ])('explains fair-launch code %i in words', (code, fragment) => {
    const view = presentSimulationFailure(hookFailure(code), { fairLaunchProgramId: fairLaunch });
    expect(view.source).toBe('fair-launch');
    expect(view.title).toBe('Swap blocked by Fair Launch');
    expect(view.reason).toContain(fragment);
    expect(view.notSubmitted).toBe(true);
  });

  it('does not guess at a code from another hook', () => {
    const view = presentSimulationFailure(hookFailure(0xb003, 'Other11111111111111111111111111111111111111'), { fairLaunchProgramId: fairLaunch });
    expect(view.source).toBe('hook');
    expect(view.reason).toContain('0xB003');
    expect(view.reason).toContain('No known mapping');
  });

  it('keeps unknown codes from the fair-launch program honest too', () => {
    const view = presentSimulationFailure(hookFailure(0xdead), { fairLaunchProgramId: fairLaunch });
    expect(view.title).toBe('Transfer Hook rejected this transaction.');
  });

  it('keeps the raw logs for the developer panel', () => {
    const view = presentSimulationFailure(hookFailure(0xb004), { fairLaunchProgramId: fairLaunch }, ['log line one']);
    expect(view.raw).toContain('log line one');
  });

  it('presents a refused hook slice as a client refusal', () => {
    const view = presentClientError(new HookClientError('unexpected-writable', 'the hook asked for X to be writable'));
    expect(view).toMatchObject({ source: 'client', rule: 'unexpected-writable', notSubmitted: true });
  });
});

describe('swap button', () => {
  const base = { connected: true, amount: 10n, balance: 100n, hasQuote: true, gated: false, busy: false };
  it('walks through its states', () => {
    expect(swapButtonState({ ...base, connected: false })).toEqual({ label: 'Connect wallet', disabled: true });
    expect(swapButtonState({ ...base, amount: null })).toEqual({ label: 'Enter an amount', disabled: true });
    expect(swapButtonState({ ...base, balance: 5n })).toEqual({ label: 'Insufficient balance', disabled: true });
    expect(swapButtonState({ ...base, gated: true }).disabled).toBe(true);
    expect(swapButtonState({ ...base, busy: true })).toEqual({ label: 'Swapping…', disabled: true });
    expect(swapButtonState(base)).toEqual({ label: 'Swap', disabled: false });
  });
});

describe('adapter selection', () => {
  const environment = {
    name: 'localnet',
    cluster: 'localnet' as const,
    rpcUrl: 'http://127.0.0.1:8899',
    cpmmProgramId: key(0xc1).toBase58(),
    clmmProgramId: key(0xc2).toBase58(),
    fairLaunchProgramId: key(0xf1).toBase58(),
  };

  it('chooses CPMM or CLMM from the pool owner and refuses any other program', () => {
    expect(adapterForOwner(environment, key(0xc1))?.kind).toBe('cpmm');
    expect(adapterForOwner(environment, key(0xc2))?.kind).toBe('clmm');
    expect(adapterForOwner(environment, key(0xc3))).toBeNull();
  });

  it('names the hook-aware instruction each adapter builds', () => {
    expect(adapterForKind('cpmm').instruction).toBe('swap_base_input_v2');
    expect(adapterForKind('clmm').instruction).toBe('swap_v3');
  });

  it('quotes a CPMM pool through the curve calculator', () => {
    const pool = poolView();
    const quote = cpmmAdapter.quote(pool, true, 1_000_000n, 50);
    expect(quote.amountOut).toBeGreaterThan(0n);
    expect(quote.tickArrays).toEqual([]);
    expect(quote.bitmapExtension).toBeNull();
  });
});
