import { describe, expect, it } from 'vitest';
import { PublicKey } from '@solana/web3.js';
import {
  type ClmmSwapAccounts,
  type CpmmSwapAccounts,
  buildClmmSwapV2,
  buildClmmSwapV3,
  buildCpmmSwapBaseInputV1,
  buildCpmmSwapBaseInputV2,
} from '../src/index.ts';
import { type Golden, hex, keyOf, legOf, loadGolden, sliceOf } from './golden.ts';

function cpmmAccounts(g: Golden): CpmmSwapAccounts {
  return {
    payer: keyOf(g, 'payer'),
    authority: keyOf(g, 'authority'),
    ammConfig: keyOf(g, 'amm_config'),
    poolState: keyOf(g, 'pool_state'),
    inputTokenAccount: keyOf(g, 'input_token_account'),
    outputTokenAccount: keyOf(g, 'output_token_account'),
    inputVault: keyOf(g, 'input_vault'),
    outputVault: keyOf(g, 'output_vault'),
    inputTokenProgram: keyOf(g, 'input_token_program'),
    outputTokenProgram: keyOf(g, 'output_token_program'),
    inputTokenMint: keyOf(g, 'input_token_mint'),
    outputTokenMint: keyOf(g, 'output_token_mint'),
    observationState: keyOf(g, 'observation_state'),
  };
}

function clmmAccounts(g: Golden): ClmmSwapAccounts {
  return {
    payer: keyOf(g, 'payer'),
    ammConfig: keyOf(g, 'amm_config'),
    poolState: keyOf(g, 'pool_state'),
    inputTokenAccount: keyOf(g, 'input_token_account'),
    outputTokenAccount: keyOf(g, 'output_token_account'),
    inputVault: keyOf(g, 'input_vault'),
    outputVault: keyOf(g, 'output_vault'),
    observationState: keyOf(g, 'observation_state'),
    tokenProgram: keyOf(g, 'token_program'),
    tokenProgram2022: keyOf(g, 'token_program_2022'),
    memoProgram: keyOf(g, 'memo_program'),
    inputVaultMint: keyOf(g, 'input_vault_mint'),
    outputVaultMint: keyOf(g, 'output_vault_mint'),
  };
}

function expectSameAccounts(actual: { pubkey: PublicKey; isSigner: boolean; isWritable: boolean }[], golden: Golden): void {
  expect(actual.map((meta) => [meta.pubkey.toBase58(), meta.isSigner, meta.isWritable])).toEqual(
    golden.accounts.map((account) => [account.pubkey, account.signer, account.writable])
  );
}

describe('CPMM against the Rust goldens', () => {
  it('builds the V1 swap_base_input byte for byte', () => {
    const golden = loadGolden('cpmm_swap_base_input_v1.json');
    const ix = buildCpmmSwapBaseInputV1(new PublicKey(golden.program_id), cpmmAccounts(golden), 1_000_000n, 900_000n);
    expect(hex(ix.data)).toBe(golden.data_hex);
    expectSameAccounts(ix.keys, golden);
  });

  it('frames swap_base_input_v2 byte for byte: data, order and flags', () => {
    const golden = loadGolden('cpmm_swap_base_input_v2.json');
    const a = cpmmAccounts(golden);
    const input = legOf(
      'input',
      { mint: a.inputTokenMint, source: a.inputTokenAccount, destination: a.inputVault, authority: a.payer, amount: 1_000_000n },
      sliceOf(golden, 'input')
    );
    const output = legOf(
      'output',
      { mint: a.outputTokenMint, source: a.outputVault, destination: a.outputTokenAccount, authority: a.authority, amount: 900_000n },
      sliceOf(golden, 'output')
    );
    const ix = buildCpmmSwapBaseInputV2(new PublicKey(golden.program_id), a, 1_000_000n, 900_000n, input, output);
    expect(hex(ix.data)).toBe(golden.data_hex);
    expect(hex(ix.data.subarray(0, 8))).toBe(golden.discriminator_hex);
    expectSameAccounts(ix.keys, golden);
    // counts are the two trailing u16s
    expect(ix.data.readUInt16LE(24)).toBe(3);
    expect(ix.data.readUInt16LE(26)).toBe(3);
  });
});

describe('CLMM against the Rust goldens', () => {
  const args = {
    amount: 1_000_000n,
    otherAmountThreshold: 900_000n,
    sqrtPriceLimitX64: 0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10n,
    isBaseInput: true,
  };

  it('builds the swap_v2 byte for byte', () => {
    const golden = loadGolden('clmm_swap_v2.json');
    const ticks = ['tick_array_0', 'tick_array_1'].map((role) => keyOf(golden, role));
    const ix = buildClmmSwapV2(new PublicKey(golden.program_id), clmmAccounts(golden), ticks, keyOf(golden, 'bitmap_extension'), args);
    expect(hex(ix.data)).toBe(golden.data_hex);
    expectSameAccounts(ix.keys, golden);
  });

  it('frames swap_v3 byte for byte: counts for ticks, bitmap, input and output', () => {
    const golden = loadGolden('clmm_swap_v3.json');
    const a = clmmAccounts(golden);
    const ticks = ['tick_array_0', 'tick_array_1'].map((role) => keyOf(golden, role));
    const input = legOf(
      'input',
      { mint: a.inputVaultMint, source: a.inputTokenAccount, destination: a.inputVault, authority: a.payer, amount: 1_000_000n },
      sliceOf(golden, 'input')
    );
    const output = legOf(
      'output',
      { mint: a.outputVaultMint, source: a.outputVault, destination: a.outputTokenAccount, authority: a.poolState, amount: 900_000n },
      sliceOf(golden, 'output')
    );
    const ix = buildClmmSwapV3(new PublicKey(golden.program_id), a, ticks, keyOf(golden, 'bitmap_extension'), args, input, output);
    expect(hex(ix.data)).toBe(golden.data_hex);
    expectSameAccounts(ix.keys, golden);
  });
});
