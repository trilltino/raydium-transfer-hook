import { type HolderRecord, readHolderRecord } from '@raydium-transfer-hook/client';
import { getAssociatedTokenAddressSync } from '@solana/spl-token';
import type { Connection, PublicKey } from '@solana/web3.js';
import { useCallback, useEffect, useState } from 'react';
import { type RewardsState, loadBalance } from '../lib/chain.ts';

export interface RewardsAccount {
  /** The wallet's associated account of the hooked token: the one that registers. */
  tokenAccount: PublicKey;
  /** `null` until that account registers. */
  record: HolderRecord | null;
  balance: bigint;
  /** The wallet's balance of the reward token. */
  rewardBalance: bigint;
}

/**
 * The connected wallet's place in a holder-rewards stream: its record (if registered), its balance of
 * the hooked token and of the reward token. Reloaded on demand, after a register or a claim.
 */
export function useRewardsAccount(
  connection: Connection,
  rewards: RewardsState | null,
  hookedMint: PublicKey | null,
  hookedTokenProgram: PublicKey | null,
  hookProgram: PublicKey | null,
  wallet: PublicKey | null
) {
  const [account, setAccount] = useState<RewardsAccount | null>(null);
  const [error, setError] = useState<string | null>(null);
  const key = `${wallet?.toBase58() ?? ''}:${hookedMint?.toBase58() ?? ''}`;

  const refresh = useCallback(async () => {
    if (!rewards || !hookedMint || !hookedTokenProgram || !hookProgram || !wallet) {
      setAccount(null);
      return;
    }
    try {
      const tokenAccount = getAssociatedTokenAddressSync(hookedMint, wallet, false, hookedTokenProgram);
      const [record, balance, rewardBalance] = await Promise.all([
        readHolderRecord(connection, tokenAccount, hookProgram),
        loadBalance(connection, wallet, hookedMint, hookedTokenProgram),
        loadBalance(connection, wallet, rewards.rewardMint.mint, rewards.rewardMint.tokenProgram),
      ]);
      setAccount({ tokenAccount, record, balance, rewardBalance });
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }, [connection, rewards, hookedMint, hookedTokenProgram, hookProgram, wallet]);

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, rewards?.global.stream.index, rewards?.global.stream.eligibleSupply]);

  return { account, error, refresh };
}
