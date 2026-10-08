import { describe, expect, it } from 'vitest';
import { PublicKey } from '@solana/web3.js';
import {
  CLMM_SWAP_V3_DISCRIMINATOR,
  CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR,
  CPMM_SWAP_BASE_OUTPUT_V2_DISCRIMINATOR,
  type CpmmSwapAccounts,
  HookClientError,
  type ResolvedLeg,
  buildClmmSwapV3,
  buildCpmmSwapBaseInputV2,
  buildCpmmSwapBaseOutputV2,
} from '../src/index.ts';
import { key } from './chain.ts';

const program = key(0xc1);
const accounts: CpmmSwapAccounts = {
  payer: key(1),
  authority: key(2),
  ammConfig: key(3),
  poolState: key(4),
  inputTokenAccount: key(5),
  outputTokenAccount: key(6),
  inputVault: key(7),
  outputVault: key(8),
  inputTokenProgram: key(9),
  outputTokenProgram: key(10),
  inputTokenMint: key(11),
  outputTokenMint: key(12),
  observationState: key(13),
};

const slice = (...keys: PublicKey[]) => keys.map((pubkey) => ({ pubkey, isSigner: false, isWritable: false }));

function legs(inputSlice = slice(key(0x71), key(0x72), key(0x73)), outputSlice = slice(key(0x74), key(0x75), key(0x76))) {
  const input: ResolvedLeg = {
    leg: { role: 'input', mint: key(11), source: key(5), destination: key(7), authority: key(1), amount: 10n },
    hookProgram: inputSlice.length ? inputSlice[inputSlice.length - 2].pubkey : null,
    slice: inputSlice,
  };
  const output: ResolvedLeg = {
    leg: { role: 'output', mint: key(12), source: key(8), destination: key(6), authority: key(2), amount: 9n },
    hookProgram: outputSlice.length ? outputSlice[outputSlice.length - 2].pubkey : null,
    slice: outputSlice,
  };
  return { input, output };
}

function kindOf(run: () => unknown): string {
  try {
    run();
  } catch (error) {
    if (error instanceof HookClientError) return error.kind;
    throw error;
  }
  return 'no error';
}

describe('CPMM framing', () => {
  it('appends the input slice then the output slice, unmerged and unsorted', () => {
    const { input, output } = legs(slice(key(0x90), key(0x72), key(0x73)), slice(key(0x70), key(0x75), key(0x76)));
    const ix = buildCpmmSwapBaseInputV2(program, accounts, 10n, 1n, input, output);
    expect(ix.keys.slice(13).map((meta) => meta.pubkey.toBase58())).toEqual(
      [0x90, 0x72, 0x73, 0x70, 0x75, 0x76].map((byte) => key(byte).toBase58())
    );
    expect(ix.data.subarray(0, 8).equals(CPMM_SWAP_BASE_INPUT_V2_DISCRIMINATOR)).toBe(true);
    expect([ix.data.readUInt16LE(24), ix.data.readUInt16LE(26)]).toEqual([3, 3]);
  });

  it('keeps a shared account in both slices when the privileges agree', () => {
    const shared = key(0x80);
    const { input, output } = legs(slice(shared, key(0x72), key(0x73)), slice(shared, key(0x75), key(0x76)));
    const ix = buildCpmmSwapBaseInputV2(program, accounts, 10n, 1n, input, output);
    expect(ix.keys.filter((meta) => meta.pubkey.equals(shared))).toHaveLength(2);
  });

  it('records a zero count for an unhooked leg', () => {
    const { input, output } = legs([], slice(key(0x74), key(0x75), key(0x76)));
    const ix = buildCpmmSwapBaseInputV2(program, accounts, 10n, 1n, input, output);
    expect([ix.data.readUInt16LE(24), ix.data.readUInt16LE(26)]).toEqual([0, 3]);
    expect(ix.keys).toHaveLength(13 + 3);
  });

  it('builds the exact-output variant with the same layout', () => {
    const { input, output } = legs();
    const ix = buildCpmmSwapBaseOutputV2(program, accounts, 20n, 9n, input, output);
    expect(ix.data.subarray(0, 8).equals(CPMM_SWAP_BASE_OUTPUT_V2_DISCRIMINATOR)).toBe(true);
    expect(ix.data.readBigUInt64LE(8)).toBe(20n);
    expect(ix.data.readBigUInt64LE(16)).toBe(9n);
  });

  it('refuses a leg that is not the transfer the swap performs', () => {
    const { input, output } = legs();
    input.leg = { ...input.leg, destination: key(0x33) };
    expect(kindOf(() => buildCpmmSwapBaseInputV2(program, accounts, 10n, 1n, input, output))).toBe('leg-mismatch');
  });

  it('refuses a slice that would escalate a fixed account', () => {
    const escalating = [{ pubkey: accounts.ammConfig, isSigner: false, isWritable: true }, ...slice(key(0x72), key(0x73))];
    const { input, output } = legs(escalating);
    expect(kindOf(() => buildCpmmSwapBaseInputV2(program, accounts, 10n, 1n, input, output))).toBe('privilege-conflict');
  });

  it('refuses the same account in both slices with different privileges', () => {
    const shared = key(0x80);
    const { input, output } = legs(
      [{ pubkey: shared, isSigner: false, isWritable: true }, ...slice(key(0x72), key(0x73))],
      slice(shared, key(0x75), key(0x76))
    );
    expect(kindOf(() => buildCpmmSwapBaseInputV2(program, accounts, 10n, 1n, input, output))).toBe('privilege-conflict');
  });

  it('refuses amounts that do not fit a u64', () => {
    const { input, output } = legs();
    expect(kindOf(() => buildCpmmSwapBaseInputV2(program, accounts, 1n << 64n, 1n, input, output))).toBe('bad-instruction-input');
    expect(kindOf(() => buildCpmmSwapBaseInputV2(program, accounts, -1n, 1n, input, output))).toBe('bad-instruction-input');
  });
});

describe('CLMM framing', () => {
  const clmm = {
    payer: key(1),
    ammConfig: key(2),
    poolState: key(3),
    inputTokenAccount: key(4),
    outputTokenAccount: key(5),
    inputVault: key(6),
    outputVault: key(7),
    observationState: key(8),
    tokenProgram: key(9),
    tokenProgram2022: key(10),
    memoProgram: key(11),
    inputVaultMint: key(12),
    outputVaultMint: key(13),
  };
  const args = { amount: 10n, otherAmountThreshold: 1n, sqrtPriceLimitX64: 0n, isBaseInput: true };

  it('signs the output leg with the pool and counts tick arrays, bitmap and slices', () => {
    const input: ResolvedLeg = {
      leg: { role: 'input', mint: key(12), source: key(4), destination: key(6), authority: key(1), amount: 10n },
      hookProgram: key(0x72),
      slice: slice(key(0x71), key(0x72), key(0x73)),
    };
    const output: ResolvedLeg = {
      leg: { role: 'output', mint: key(13), source: key(7), destination: key(5), authority: key(3), amount: 9n },
      hookProgram: null,
      slice: [],
    };
    const ix = buildClmmSwapV3(program, clmm, [key(0x21), key(0x22)], null, args, input, output);
    expect(ix.data.subarray(0, 8).equals(CLMM_SWAP_V3_DISCRIMINATOR)).toBe(true);
    const counts = [41, 43, 45, 47].map((offset) => ix.data.readUInt16LE(offset));
    expect(counts).toEqual([2, 0, 3, 0]);
    expect(ix.keys).toHaveLength(13 + 2 + 3);
  });

  it('refuses an output leg authorised by anything but the pool', () => {
    const input: ResolvedLeg = {
      leg: { role: 'input', mint: key(12), source: key(4), destination: key(6), authority: key(1), amount: 10n },
      hookProgram: null,
      slice: [],
    };
    const output: ResolvedLeg = {
      leg: { role: 'output', mint: key(13), source: key(7), destination: key(5), authority: key(2), amount: 9n },
      hookProgram: null,
      slice: [],
    };
    expect(kindOf(() => buildClmmSwapV3(program, clmm, [], null, args, input, output))).toBe('leg-mismatch');
  });
});
