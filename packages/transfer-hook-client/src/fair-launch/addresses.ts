import { PublicKey } from '@solana/web3.js';

const seed = (text: string): Buffer => Buffer.from(text, 'utf8');

/** The config PDA of `mint`: seeds `["config", mint]` under the fair-launch program. */
export function getFairLaunchConfigAddress(mint: PublicKey, programId: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync([seed('config'), mint.toBuffer()], programId)[0];
}

/** The slot-counter PDA of `mint`: seeds `["counter", mint]`. */
export function getFairLaunchCounterAddress(mint: PublicKey, programId: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync([seed('counter'), mint.toBuffer()], programId)[0];
}
