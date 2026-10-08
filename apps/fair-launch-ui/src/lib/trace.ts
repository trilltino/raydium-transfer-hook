import type { HookEnvironment } from '@raydium-transfer-hook/client';
import type { ParsedInstruction, ParsedTransactionWithMeta, PartiallyDecodedInstruction } from '@solana/web3.js';

/**
 * Rebuild what a landed transaction did, program by program, from its `getTransaction` answer
 * (`jsonParsed`, so the Token-2022 transfers arrive decoded). The runtime's logs list every program
 * invocation in the order it ran, with its depth, its compute and its own log lines; the transaction's
 * outer instructions followed by each one's inner instructions list the same invocations in the same order,
 * so the n-th invocation in the logs is the n-th instruction in the flattened list. The approach is the one
 * `raydium_debugger` uses for its execution tree.
 */

export interface TraceStep {
  id: string;
  /** 1-based position in the order the runtime ran the programs. */
  number: number;
  /** 1 for an instruction of the transaction, 2 for a program it called, and so on. */
  depth: number;
  programId: string;
  programName: string;
  /** What this program was asked to do, when it can be named. */
  instruction: string | null;
  failed: boolean;
  computeUnits: number | null;
  /** The accounts the instruction named, in order (for a decoded instruction, the addresses in its fields). */
  accounts: string[];
  /** The log lines this program wrote itself, not those of the programs it called. */
  logs: string[];
  /** The decoded fields of a Token-2022 or System instruction. */
  details: { name: string; value: string }[];
  /** The step is a Transfer Hook program running inside a Token-2022 transfer. */
  isHook: boolean;
}

export interface TraceView {
  signature: string;
  slot: number;
  success: boolean;
  error: string | null;
  feeLamports: number;
  computeUnits: number | null;
  /** The compute-unit limit the transaction asked for, if it set one. */
  computeLimit: number | null;
  /** Where the transaction was read from, e.g. "Triton One (devnet)". */
  source: string;
  /** Unix time of the block, if the RPC gave one. */
  blockTime: number | null;
  /** The priority fee the transaction declared, in micro-lamports per compute unit. */
  computeUnitPrice: number | null;
  /** Each token account's raw balance after the transaction, by address. */
  postBalances: Record<string, string>;
  steps: TraceStep[];
  /** How many times each hook program ran, for the "the hook ran N times" line. */
  hookRuns: { programId: string; programName: string; count: number }[];
}

type AnyInstruction = ParsedInstruction | PartiallyDecodedInstruction;

const ALPHABET = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

/** Decode base58 (instruction data in a `getTransaction` answer). */
export function base58Decode(text: string): Uint8Array {
  const bytes: number[] = [];
  for (const char of text) {
    let carry = ALPHABET.indexOf(char);
    if (carry < 0) throw new Error(`not base58: ${char}`);
    for (let i = 0; i < bytes.length; i += 1) {
      carry += bytes[i] * 58;
      bytes[i] = carry & 0xff;
      carry >>= 8;
    }
    while (carry > 0) {
      bytes.push(carry & 0xff);
      carry >>= 8;
    }
  }
  for (const char of text) {
    if (char !== '1') break;
    bytes.push(0);
  }
  return Uint8Array.from(bytes.reverse());
}

const hex = (bytes: Uint8Array): string => [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');

/** spl-transfer-hook-interface `Execute`, as the first eight bytes of its data in hex. */
const EXECUTE = '692565c54bfb661a';

/** The Anchor discriminators of the instructions this page builds (the first eight bytes of the data, in hex). */
const INSTRUCTIONS: Record<string, string> = {
  '8fbe5adac41e33de': 'swap_base_input',
  b387d1d9874b283a: 'swap_base_input_v2',
  '37d96256a34ab4ad': 'swap_base_output',
  '1d8fdf6d036f9793': 'swap_base_output_v2',
  '2b04ed0b1ac91e62': 'swap_v2',
  f0e02621b01ff1af: 'swap_v3',
  [EXECUTE]: 'Execute (Transfer Hook)',
};

/** The holder-rewards hook's own instructions are told apart by their first byte. */
const HOLDER_REWARDS_TAGS: Record<string, string> = { '01': 'Register', '03': 'Claim' };

const COMPUTE_BUDGET = 'ComputeBudget111111111111111111111111111111';
const WELL_KNOWN: Record<string, string> = {
  '11111111111111111111111111111111': 'System Program',
  [COMPUTE_BUDGET]: 'Compute Budget',
  TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA: 'SPL Token',
  TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb: 'Token-2022',
  ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL: 'Associated Token',
  MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr: 'Memo',
};

export interface ProgramNames {
  names: Map<string, string>;
  hooks: Set<string>;
}

/** The programs this environment knows by name. */
export function programNames(environment: HookEnvironment): ProgramNames {
  const names = new Map<string, string>(Object.entries(WELL_KNOWN));
  names.set(environment.cpmmProgramId, 'Raydium CPMM (hook-aware fork)');
  names.set(environment.clmmProgramId, 'Raydium CLMM (hook-aware fork)');
  const hooks = new Set<string>();
  const hook = (id: string | undefined, name: string) => {
    if (!id) return;
    names.set(id, name);
    hooks.add(id);
  };
  hook(environment.fairLaunchProgramId, 'Fair Launch hook');
  hook(environment.creatorCommitmentProgramId, 'Creator Commitment hook');
  hook(environment.holderRewardsProgramId, 'Holder Rewards hook');
  return { names, hooks };
}

const looksLikeAddress = (value: unknown): value is string => typeof value === 'string' && /^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(value);
const snake = (text: string): string => text.replace(/([A-Z])/g, '_$1').toLowerCase();

interface Flat {
  programId: string;
  instruction: string | null;
  accounts: string[];
  details: { name: string; value: string }[];
  discriminator: string | null;
  data: Uint8Array | null;
}

function flatten(instruction: AnyInstruction): Flat {
  const programId = instruction.programId.toBase58();
  if ('parsed' in instruction) {
    const parsed = instruction.parsed as { type?: string; info?: Record<string, unknown> };
    const info = parsed.info ?? {};
    const details: { name: string; value: string }[] = [];
    const accounts: string[] = [];
    for (const [name, value] of Object.entries(info)) {
      if (name === 'tokenAmount' && value && typeof value === 'object') {
        details.push({ name: 'amount', value: String((value as { amount?: string }).amount ?? '') });
      } else if (looksLikeAddress(value)) {
        details.push({ name, value });
        accounts.push(value);
      } else if (typeof value === 'string' || typeof value === 'number') {
        details.push({ name, value: String(value) });
      }
    }
    return { programId, instruction: parsed.type ? snake(parsed.type) : null, accounts, details, discriminator: null, data: null };
  }
  const data = base58Decode(instruction.data);
  return {
    programId,
    instruction: null,
    accounts: instruction.accounts.map((key) => key.toBase58()),
    details: [],
    discriminator: data.length > 0 ? hex(data.subarray(0, 8)) : null,
    data,
  };
}

interface Invocation {
  programId: string;
  depth: number;
  logs: string[];
  computeUnits: number | null;
  failed: boolean;
}

/** The invocations in the order the runtime ran them, from the transaction's own log lines. */
export function invocationsFromLogs(logs: readonly string[]): Invocation[] {
  const all: Invocation[] = [];
  const stack: Invocation[] = [];
  for (const line of logs) {
    const invoke = /^Program (\S+) invoke \[(\d+)\]$/.exec(line);
    if (invoke) {
      const node: Invocation = { programId: invoke[1], depth: Number(invoke[2]), logs: [], computeUnits: null, failed: false };
      all.push(node);
      stack.push(node);
      continue;
    }
    const top = stack[stack.length - 1];
    if (!top) continue;
    const consumed = /^Program (\S+) consumed (\d+) of \d+ compute units$/.exec(line);
    if (consumed && consumed[1] === top.programId) {
      top.computeUnits = Number(consumed[2]);
      continue;
    }
    if (line === `Program ${top.programId} success`) {
      stack.pop();
      continue;
    }
    if (line.startsWith(`Program ${top.programId} failed`)) {
      top.failed = true;
      top.logs.push(line);
      stack.pop();
      continue;
    }
    top.logs.push(line);
  }
  return all;
}

/** Turn a landed (or failed) transaction into the steps the page shows. */
export function buildTrace(transaction: ParsedTransactionWithMeta, environment: HookEnvironment, source: string): TraceView {
  const { names, hooks } = programNames(environment);
  const meta = transaction.meta;
  const outer = transaction.transaction.message.instructions;
  const inner = new Map((meta?.innerInstructions ?? []).map((group) => [group.index, group.instructions]));
  const flat: Flat[] = [];
  outer.forEach((instruction, index) => {
    flat.push(flatten(instruction));
    for (const nested of inner.get(index) ?? []) flat.push(flatten(nested));
  });
  const invocations = invocationsFromLogs(meta?.logMessages ?? []);
  const signature = transaction.transaction.signatures[0] ?? '';

  let computeLimit: number | null = null;
  let computeUnitPrice: number | null = null;
  const steps: TraceStep[] = invocations.map((invocation, index) => {
    // The log of a very long transaction can be cut short; a step with no instruction still shows its program.
    const instruction = flat[index]?.programId === invocation.programId ? flat[index] : undefined;
    const discriminator = instruction?.discriminator ?? null;
    const isHook = hooks.has(invocation.programId) || (invocation.depth > 1 && discriminator === EXECUTE);
    const short = `${invocation.programId.slice(0, 4)}…${invocation.programId.slice(-4)}`;
    if (invocation.programId === COMPUTE_BUDGET && instruction?.data && instruction.data[0] === 2 && instruction.data.length >= 5) {
      computeLimit = new DataView(instruction.data.buffer, instruction.data.byteOffset + 1, 4).getUint32(0, true);
    }
    if (invocation.programId === COMPUTE_BUDGET && instruction?.data && instruction.data[0] === 3 && instruction.data.length >= 9) {
      computeUnitPrice = Number(new DataView(instruction.data.buffer, instruction.data.byteOffset + 1, 8).getBigUint64(0, true));
    }
    const name =
      (instruction?.instruction ?? null) ||
      (discriminator ? INSTRUCTIONS[discriminator] : undefined) ||
      (discriminator && invocation.programId === environment.holderRewardsProgramId ? HOLDER_REWARDS_TAGS[discriminator.slice(0, 2)] : undefined) ||
      (invocation.programId === COMPUTE_BUDGET ? computeBudgetName(instruction?.data) : undefined) ||
      null;
    return {
      id: `step-${index + 1}`,
      number: index + 1,
      depth: invocation.depth,
      programId: invocation.programId,
      programName: names.get(invocation.programId) ?? (isHook ? `Transfer Hook ${short}` : `Program ${short}`),
      instruction: name,
      failed: invocation.failed,
      computeUnits: invocation.computeUnits,
      accounts: instruction?.accounts ?? [],
      logs: invocation.logs,
      details: instruction?.details ?? [],
      isHook,
    };
  });

  const runs = new Map<string, { programId: string; programName: string; count: number }>();
  for (const step of steps) {
    if (!step.isHook) continue;
    const entry = runs.get(step.programId) ?? { programId: step.programId, programName: step.programName, count: 0 };
    entry.count += 1;
    runs.set(step.programId, entry);
  }

  const keys = transaction.transaction.message.accountKeys;
  const postBalances: Record<string, string> = {};
  for (const balance of meta?.postTokenBalances ?? []) {
    const key = keys[balance.accountIndex]?.pubkey.toBase58();
    if (key) postBalances[key] = balance.uiTokenAmount.amount;
  }

  return {
    signature,
    slot: transaction.slot,
    success: !meta?.err,
    error: meta?.err ? JSON.stringify(meta.err) : null,
    feeLamports: meta?.fee ?? 0,
    computeUnits: meta?.computeUnitsConsumed ?? null,
    computeLimit,
    computeUnitPrice,
    blockTime: transaction.blockTime ?? null,
    postBalances,
    source,
    steps,
    hookRuns: [...runs.values()],
  };
}

function computeBudgetName(data: Uint8Array | null | undefined): string | undefined {
  switch (data?.[0]) {
    case 2:
      return 'set_compute_unit_limit';
    case 3:
      return 'set_compute_unit_price';
    case 4:
      return 'set_loaded_accounts_data_size_limit';
    default:
      return undefined;
  }
}
