import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { type Adapter, WalletAdapterNetwork } from '@solana/wallet-adapter-base';
import { PhantomWalletAdapter } from '@solana/wallet-adapter-phantom';
import { SolflareWalletAdapter } from '@solana/wallet-adapter-solflare';

/**
 * The network the wallets are told the page is on. Solflare compares it with the network chosen in the wallet and
 * refuses to sign on a mismatch ("this transaction is for mainnet"), and its adapter assumes mainnet unless told
 * otherwise. This is a devnet page; a local validator is a development setup that has no wallet network of its own,
 * so it uses devnet's too.
 */
export function networkFor(_environment: HookEnvironment): WalletAdapterNetwork {
  return WalletAdapterNetwork.Devnet;
}

/** Phantom and Solflare by name; any other Wallet Standard extension is added by the wallet provider itself. */
export function walletAdapters(environment: HookEnvironment): Adapter[] {
  return [new PhantomWalletAdapter(), new SolflareWalletAdapter({ network: networkFor(environment) })];
}
