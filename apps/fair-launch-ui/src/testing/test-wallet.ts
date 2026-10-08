import {
  BaseMessageSignerWalletAdapter,
  type WalletName,
  WalletReadyState,
} from '@solana/wallet-adapter-base';
import { Keypair, type PublicKey, type Transaction, type VersionedTransaction } from '@solana/web3.js';

/**
 * A wallet for the browser end-to-end test only. It signs with a throwaway key handed in through the page
 * URL, so it is created exclusively in builds made with `VITE_E2E=1`; production builds never import it.
 */
export class TestWalletAdapter extends BaseMessageSignerWalletAdapter {
  name = 'E2E Test Wallet' as WalletName<'E2E Test Wallet'>;
  url = 'https://example.invalid';
  icon = 'data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciLz4=';
  supportedTransactionVersions = new Set(['legacy', 0] as const);
  readyState = WalletReadyState.Loadable;
  private keypair: Keypair;
  private connectedKey: PublicKey | null = null;

  constructor(secret: Uint8Array) {
    super();
    this.keypair = Keypair.fromSecretKey(secret);
  }

  get publicKey(): PublicKey | null {
    return this.connectedKey;
  }
  get connecting(): boolean {
    return false;
  }

  async connect(): Promise<void> {
    this.connectedKey = this.keypair.publicKey;
    this.emit('connect', this.keypair.publicKey);
  }
  async disconnect(): Promise<void> {
    this.connectedKey = null;
    this.emit('disconnect');
  }
  async signTransaction<T extends Transaction | VersionedTransaction>(transaction: T): Promise<T> {
    if ('version' in transaction) transaction.sign([this.keypair]);
    else transaction.partialSign(this.keypair);
    return transaction;
  }
  async signMessage(message: Uint8Array): Promise<Uint8Array> {
    return message;
  }
}
