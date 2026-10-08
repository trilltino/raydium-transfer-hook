import type { HookEnvironment } from '@raydium-transfer-hook/client';
import { type Connection, LAMPORTS_PER_SOL, type PublicKey } from '@solana/web3.js';
import { useCallback, useEffect, useState } from 'react';

/** Below this many lamports a wallet cannot be relied on to pay for a swap and the token accounts it creates. */
export const LOW_SOL_LAMPORTS = 20_000_000;
const AIRDROP_SOL = 1;

export interface GetSolProps {
  environment: HookEnvironment;
  connection: Connection;
  wallet: PublicKey;
  /** Called after the SOL arrived, so balances are read again. */
  onFunded?: () => void;
}

type State = { phase: 'idle' } | { phase: 'busy' } | { phase: 'done' } | { phase: 'error'; message: string };

/**
 * Shown while the connected wallet holds almost no SOL on this network: a swap needs SOL for fees, and without
 * any the network reports the wallet as not found. The airdrop is the network's own faucet, asked from the page
 * (a local validator hands SOL out freely; devnet's faucet is rate-limited and may say no).
 */
export function GetSol({ environment, connection, wallet, onFunded }: GetSolProps) {
  const [lamports, setLamports] = useState<number | null>(null);
  const [state, setState] = useState<State>({ phase: 'idle' });
  const walletKey = wallet.toBase58();

  const read = useCallback(async () => {
    try {
      setLamports(await connection.getBalance(wallet, 'confirmed'));
    } catch {
      setLamports(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connection, walletKey]);

  useEffect(() => {
    void read();
  }, [read]);

  if (lamports === null || lamports >= LOW_SOL_LAMPORTS) return null;

  const run = async () => {
    setState({ phase: 'busy' });
    try {
      const signature = await connection.requestAirdrop(wallet, AIRDROP_SOL * LAMPORTS_PER_SOL);
      const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash('confirmed');
      await connection.confirmTransaction({ signature, blockhash, lastValidBlockHeight }, 'confirmed');
      setState({ phase: 'done' });
      await read();
      onFunded?.();
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      setState({
        phase: 'error',
        message:
          environment.cluster === 'devnet'
            ? `The devnet faucet said no (${reason}). It is rate-limited: try https://faucet.solana.com with this wallet address.`
            : `The airdrop failed (${reason}).`,
      });
    }
  };

  return (
    <div className="test-tokens-row" data-testid="get-sol">
      <button type="button" className="btn btn-ghost" disabled={state.phase === 'busy'} onClick={() => void run()}>
        {state.phase === 'busy' ? 'Requesting…' : `Get ${AIRDROP_SOL} SOL`}
      </button>
      <span className="muted small">
        This wallet has {(lamports / LAMPORTS_PER_SOL).toFixed(3)} SOL on {environment.cluster === 'devnet' ? 'devnet' : 'the local validator'}, so it cannot pay fees.
      </span>
      {state.phase === 'error' && (
        <p className="notice notice-error" role="alert" data-testid="get-sol-error">
          {state.message}
        </p>
      )}
    </div>
  );
}
