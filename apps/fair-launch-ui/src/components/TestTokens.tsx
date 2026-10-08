import type { HookEnvironment } from '@raydium-transfer-hook/client';
import {
  createAssociatedTokenAccountIdempotentInstruction,
  createMintToInstruction,
  getAssociatedTokenAddressSync,
} from '@solana/spl-token';
import { useWallet } from '@solana/wallet-adapter-react';
import type { Connection, PublicKey } from '@solana/web3.js';
import { useEffect, useState } from 'react';
import { faucetAvailable, requestTokens } from '../lib/faucet-client.ts';
import { type PoolTokenInfo, mintedByWallet } from '../lib/mint-authority.ts';
import { presentContext } from '../lib/present.ts';
import { runTransaction } from '../lib/run-transaction.ts';
import { solscanUrl } from '../lib/solscan.ts';

const TOKENS = 100;

export interface TestTokensProps {
  environment: HookEnvironment;
  connection: Connection;
  wallet: PublicKey;
  tokens: PoolTokenInfo[];
  /** Called after tokens were minted, so the balances are read again. */
  onFunded: () => void;
}

type State =
  | { phase: 'idle' }
  | { phase: 'busy' }
  | { phase: 'done'; message: string; signature: string | null }
  | { phase: 'error'; message: string };

/**
 * Two ways to get some of this pool's tokens, neither of which needs a key in the browser:
 * - **Mint to my wallet**, when the connected wallet is the mint authority of a token (the creator of your own
 *   hooked token): the page builds the mint, simulates it, and your wallet signs it.
 * - **Get test tokens**, when the dev server has a faucet: it mints our demo tokens with the key that created them.
 */
export function TestTokens({ environment, connection, wallet, tokens, onFunded }: TestTokensProps) {
  const { signTransaction } = useWallet();
  const [faucet, setFaucet] = useState(false);
  const [own, setOwn] = useState<Set<string>>(new Set());
  const [ownState, setOwnState] = useState<State>({ phase: 'idle' });
  const [faucetState, setFaucetState] = useState<State>({ phase: 'idle' });
  const walletKey = wallet.toBase58();
  const mintKeys = tokens.map((token) => token.mint.toBase58()).join(',');

  useEffect(() => {
    let cancelled = false;
    void faucetAvailable(environment).then((ok) => !cancelled && setFaucet(ok));
    void mintedByWallet(connection, wallet, tokens).then((set) => !cancelled && setOwn(set));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [environment, connection, walletKey, mintKeys]);

  const mine = tokens.filter((token) => own.has(token.mint.toBase58()));
  if (!faucet && mine.length === 0) return null;

  const mintMine = async () => {
    if (!signTransaction) return setOwnState({ phase: 'error', message: 'This wallet cannot sign transactions.' });
    setOwnState({ phase: 'busy' });
    const instructions = mine.flatMap((token) => {
      const account = getAssociatedTokenAddressSync(token.mint, wallet, false, token.tokenProgram);
      return [
        createAssociatedTokenAccountIdempotentInstruction(wallet, account, wallet, token.mint, token.tokenProgram),
        createMintToInstruction(token.mint, account, wallet, BigInt(TOKENS) * 10n ** BigInt(token.decimals), [], token.tokenProgram),
      ];
    });
    const outcome = await runTransaction({
      connection,
      payer: wallet,
      instructions,
      hookPrograms: [],
      signTransaction,
      present: presentContext(environment, 'action'),
    });
    if (outcome.status === 'success') {
      setOwnState({
        phase: 'done',
        message: `Minted ${TOKENS} of ${mine.length} token${mine.length === 1 ? '' : 's'} you are the mint authority of.`,
        signature: outcome.signature,
      });
      onFunded();
    } else {
      setOwnState({ phase: 'error', message: `${outcome.failure.title}: ${outcome.failure.reason}` });
    }
  };

  const fromFaucet = async () => {
    setFaucetState({ phase: 'busy' });
    try {
      const answer = await requestTokens(environment, walletKey, tokens.map((token) => token.mint.toBase58()), TOKENS);
      const minted = answer.results.filter((result) => result.status === 'minted').length;
      const skipped = answer.results.filter((result) => result.status === 'skipped');
      const message =
        minted > 0
          ? `Minted ${TOKENS} of ${minted} token${minted === 1 ? '' : 's'} into your wallet.${skipped.length > 0 ? ` Skipped ${skipped.length}: ${skipped[0].reason}.` : ''}`
          : `Nothing minted: ${skipped.map((result) => result.reason).join('; ')}.`;
      setFaucetState({ phase: 'done', message, signature: answer.signature });
      if (minted > 0) onFunded();
    } catch (error) {
      setFaucetState({ phase: 'error', message: error instanceof Error ? error.message : String(error) });
    }
  };

  const result = (state: State, id: string) => (
    <>
      {state.phase === 'done' && (
        <p className="muted small" role="status" data-testid={`${id}-status`}>
          {state.message}{' '}
          {state.signature && (
            <a href={solscanUrl(environment, 'tx', state.signature)} target="_blank" rel="noreferrer">
              Solscan ↗
            </a>
          )}
        </p>
      )}
      {state.phase === 'error' && (
        <p className="notice notice-error" role="alert" data-testid={`${id}-error`}>
          {state.message}
        </p>
      )}
    </>
  );

  return (
    <div className="test-tokens" data-testid="test-tokens">
      {mine.length > 0 && (
        <div className="test-tokens-row">
          <button type="button" className="btn btn-ghost" disabled={ownState.phase === 'busy'} onClick={() => void mintMine()}>
            {ownState.phase === 'busy' ? 'Minting…' : `Mint ${TOKENS} to my wallet`}
          </button>
          <span className="muted small">You are the mint authority of {mine.length === tokens.length ? 'both tokens' : 'one token'}: your wallet signs the mint.</span>
          {result(ownState, 'test-tokens-own')}
        </div>
      )}
      {faucet && mine.length < tokens.length && (
        <div className="test-tokens-row">
          <button type="button" className="btn btn-ghost" disabled={faucetState.phase === 'busy'} onClick={() => void fromFaucet()}>
            {faucetState.phase === 'busy' ? 'Minting…' : 'Get test tokens'}
          </button>
          <span className="muted small">The dev server mints {TOKENS} of each demo token into your wallet ({environment.cluster} only).</span>
          {result(faucetState, 'test-tokens')}
        </div>
      )}
    </div>
  );
}
