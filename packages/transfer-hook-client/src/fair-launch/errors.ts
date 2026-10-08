/** What a fair-launch error code means, in words a trader can read. Codes are the template's `error.rs`. */
export interface FairLaunchErrorInfo {
  code: number;
  name: string;
  /** Shown to the trader. */
  message: string;
  /** True for a rule the trade broke; false for a launch that is misconfigured. */
  tradeRule: boolean;
}

const ERRORS: readonly FairLaunchErrorInfo[] = [
  { code: 0xb001, name: 'InvalidParams', message: 'The launch settings are invalid.', tradeRule: false },
  { code: 0xb002, name: 'PoolVaultMismatch', message: 'A launch venue is not a token account of this token.', tradeRule: false },
  { code: 0xb003, name: 'PerBuyCapExceeded', message: 'This buy is larger than the launch max-buy limit.', tradeRule: true },
  { code: 0xb004, name: 'MaxWalletExceeded', message: 'Your wallet balance would exceed the configured launch limit.', tradeRule: true },
  { code: 0xb005, name: 'TooManyBuysInSlot', message: 'The launch allows no more buys in this slot. Try again in a moment.', tradeRule: true },
  { code: 0xb006, name: 'PriorityFeeTooHigh', message: 'The transaction pays a priority fee above the launch limit.', tradeRule: true },
  { code: 0xb007, name: 'InvalidConfig', message: 'The launch configuration account is not the expected one.', tradeRule: false },
  { code: 0xb008, name: 'InvalidInstruction', message: 'The launch setup instruction was malformed.', tradeRule: false },
  { code: 0xb009, name: 'InvalidCounter', message: 'The launch slot counter account is wrong.', tradeRule: false },
  { code: 0xb00a, name: 'InvalidSysvar', message: 'The instructions sysvar account is missing or wrong.', tradeRule: false },
  { code: 0xb00b, name: 'InvalidVenues', message: 'The launch venues are invalid (none, more than four, or repeated).', tradeRule: false },
];

const BY_CODE = new Map(ERRORS.map((info) => [info.code, info]));

/** The fair-launch meaning of a custom program error code, or `null` if the code is not one of this template's. */
export function decodeFairLaunchError(code: number): FairLaunchErrorInfo | null {
  return BY_CODE.get(code) ?? null;
}

export const FAIR_LAUNCH_ERRORS: readonly FairLaunchErrorInfo[] = ERRORS;
