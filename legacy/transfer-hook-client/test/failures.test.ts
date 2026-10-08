import { describe, expect, it } from 'vitest';
import {
  decodeTransferHookFailure,
  maximumAmountIn,
  minimumAmountOut,
  priceImpactBps,
} from '../src/index.ts';
import { key } from './chain.ts';

const hookId = key(0x42);

describe('decodeTransferHookFailure', () => {
  const logs = (code: string) => [
    'Program Raydium invoke [1]',
    `Program Token2022 invoke [2]`,
    `Program ${hookId.toBase58()} invoke [3]`,
    `Program ${hookId.toBase58()} failed: custom program error: ${code}`,
    `Program Token2022 failed: custom program error: ${code}`,
    `Program Raydium failed: custom program error: ${code}`,
  ];

  it('attributes the failure to the innermost program, the hook', () => {
    const failure = decodeTransferHookFailure({ err: { InstructionError: [0, { Custom: 0xb004 }] }, logs: logs('0xb004') }, [hookId]);
    expect(failure).toMatchObject({ kind: 'hook', programId: hookId.toBase58(), code: 0xb004, codeHex: '0xB004' });
  });

  it('calls a failure of another program a program failure', () => {
    const failure = decodeTransferHookFailure(
      { err: {}, logs: ['Program Raydium failed: custom program error: 0x1771'] },
      [hookId]
    );
    expect(failure).toMatchObject({ kind: 'program', code: 0x1771 });
  });

  it('falls back to the structured error when there are no logs', () => {
    const failure = decodeTransferHookFailure({ err: { InstructionError: [2, { Custom: 6001 }] }, logs: null }, [hookId]);
    expect(failure).toMatchObject({ kind: 'program', code: 6001, programId: 'unknown' });
  });

  it('reports anything else as other', () => {
    expect(decodeTransferHookFailure({ err: 'BlockhashNotFound' }, [hookId])).toEqual({ kind: 'other', message: 'BlockhashNotFound' });
  });
});

describe('slippage helpers', () => {
  it('rounds the minimum down and the maximum up', () => {
    expect(minimumAmountOut(1_000n, 50)).toBe(995n);
    expect(minimumAmountOut(999n, 50)).toBe(994n);
    expect(maximumAmountIn(1_000n, 50)).toBe(1_005n);
    expect(maximumAmountIn(999n, 50)).toBe(1_004n);
  });

  it('rejects slippage outside 0..10000 bps', () => {
    expect(() => minimumAmountOut(1n, -1)).toThrow();
    expect(() => minimumAmountOut(1n, 10_001)).toThrow();
  });

  it('measures price impact against the spot price', () => {
    expect(priceImpactBps(100n, 99n, 10_000n, 10_000n)).toBe(100);
    expect(priceImpactBps(0n, 0n, 1n, 1n)).toBe(0);
  });
});
