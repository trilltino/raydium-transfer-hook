import { PublicKey } from '@solana/web3.js';

export type HookClientErrorKind =
  | 'mint-missing'
  | 'unsupported-token-program'
  | 'hook-program-mismatch'
  | 'hook-program-invalid'
  | 'validation-list-missing'
  | 'validation-list-owner'
  | 'unexpected-signer'
  | 'unexpected-writable'
  | 'bad-slice-tail'
  | 'leg-mismatch'
  | 'privilege-conflict'
  | 'bad-instruction-input'
  | 'environment';

/** A refusal made before anything is sent: the client checks what it is about to put in a transaction. */
export class HookClientError extends Error {
  constructor(
    public readonly kind: HookClientErrorKind,
    message: string
  ) {
    super(message);
    this.name = 'HookClientError';
  }
}

export type HookFailure =
  /** The Transfer Hook program itself refused the transfer, with its own error code. */
  | { kind: 'hook'; programId: string; code: number; codeHex: string }
  /** Some other program (Raydium, Token-2022) failed with a custom error code. */
  | { kind: 'program'; programId: string; code: number; codeHex: string }
  | { kind: 'other'; message: string };

const FAILED_LINE = /^Program (\w+) failed: custom program error: (0x[0-9a-fA-F]+)/;

const hex = (code: number): string => `0x${code.toString(16).toUpperCase()}`;

/**
 * Work out who refused a simulated transaction. A failure propagates outwards (the hook fails, then
 * Token-2022, then Raydium), so the first `failed: custom program error` line in the logs is the
 * innermost program: the one that really said no. If that program is one of `hookPrograms` the
 * failure is the hook's, whatever the outer programs report.
 */
export function decodeTransferHookFailure(
  input: { err: unknown; logs?: readonly string[] | null },
  hookPrograms: readonly PublicKey[]
): HookFailure {
  const hooks = new Set(hookPrograms.map((key) => key.toBase58()));
  for (const line of input.logs ?? []) {
    const match = FAILED_LINE.exec(line);
    if (!match) continue;
    const code = Number.parseInt(match[2], 16);
    const programId = match[1];
    return hooks.has(programId)
      ? { kind: 'hook', programId, code, codeHex: hex(code) }
      : { kind: 'program', programId, code, codeHex: hex(code) };
  }
  // No log line: fall back to the structured error, which does not say which program.
  const custom = customCodeOf(input.err);
  if (custom !== null) return { kind: 'program', programId: 'unknown', code: custom, codeHex: hex(custom) };
  return { kind: 'other', message: typeof input.err === 'string' ? input.err : JSON.stringify(input.err ?? 'unknown error') };
}

function customCodeOf(err: unknown): number | null {
  if (typeof err !== 'object' || err === null) return null;
  const instructionError = (err as { InstructionError?: unknown }).InstructionError;
  if (!Array.isArray(instructionError)) return null;
  const detail = instructionError[1];
  if (typeof detail === 'object' && detail !== null && 'Custom' in detail) {
    const code = (detail as { Custom: unknown }).Custom;
    return typeof code === 'number' ? code : null;
  }
  return null;
}
