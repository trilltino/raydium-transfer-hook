import { Raydium } from '@raydium-io/raydium-sdk-v2';
import {
  type FairLaunchConfig,
  type FairLaunchCounter,
  type HookEnvironment,
  type TransferHookInfo,
  programKeys,
  readFairLaunchConfig,
  readFairLaunchCounter,
  readTransferHook,
} from '@raydium-transfer-hook/client';
import { getAssociatedTokenAddressSync } from '@solana/spl-token';
import { Connection, PublicKey } from '@solana/web3.js';
import { adapterForOwner } from '../adapters/index.ts';
import type { HookAwareSwapAdapter } from '../adapters/index.ts';
import type { PoolView } from './pool.ts';

export function connectionFor(environment: HookEnvironment): Connection {
  return new Connection(environment.rpcUrl, 'confirmed');
}

/**
 * Raydium SDK V2 initialised for reading: no token-list download and no feature check, so the page works
 * against a local validator and a private deployment. The SDK is used for pool data and quote math only;
 * it never builds the hooked transaction.
 */
export async function loadRaydium(connection: Connection, environment: HookEnvironment): Promise<Raydium> {
  return Raydium.load({
    connection,
    cluster: environment.cluster === 'devnet' ? 'devnet' : 'mainnet',
    disableLoadToken: true,
    disableFeatureCheck: true,
    owner: undefined,
  } as never);
}

export interface LaunchState {
  config: FairLaunchConfig;
  counter: FairLaunchCounter | null;
  hookedSide: 'A' | 'B';
}

/** The pool is not one this environment's Raydium programs own; the page fails closed. */
export class PoolRefused extends Error {}

export interface PoolContext {
  adapter: HookAwareSwapAdapter;
  pool: PoolView;
  hookA: TransferHookInfo;
  hookB: TransferHookInfo;
  launch: LaunchState | null;
  slot: bigint;
}

/** Everything the page needs about one pool, loaded together so the numbers agree. */
export async function loadPoolContext(
  connection: Connection,
  raydium: Raydium,
  environment: HookEnvironment,
  poolId: PublicKey
): Promise<PoolContext> {
  const account = await connection.getAccountInfo(poolId, 'confirmed');
  if (account === null) throw new Error('the pool account does not exist on this cluster');
  const adapter = adapterForOwner(environment, account.owner);
  if (!adapter) {
    throw new PoolRefused(
      `This pool belongs to ${account.owner.toBase58()}, not this environment's Raydium programs ` +
        `(CPMM ${environment.cpmmProgramId}, CLMM ${environment.clmmProgramId}).`
    );
  }
  const pool = await adapter.loadPool({ connection, raydium, environment }, poolId);
  const programs = programKeys(environment);
  const [hookA, hookB, slot] = await Promise.all([
    readTransferHook(connection, pool.tokenA.mint),
    readTransferHook(connection, pool.tokenB.mint),
    connection.getSlot('confirmed'),
  ]);
  let launch: LaunchState | null = null;
  for (const [side, hook] of [['A', hookA], ['B', hookB]] as const) {
    if (!hook.hookProgramId?.equals(programs.fairLaunch)) continue;
    const mint = side === 'A' ? pool.tokenA.mint : pool.tokenB.mint;
    const config = await readFairLaunchConfig(connection, mint, programs.fairLaunch);
    if (config) {
      launch = { config, counter: await readFairLaunchCounter(connection, mint, programs.fairLaunch), hookedSide: side };
      break;
    }
  }
  return { adapter, pool, hookA, hookB, launch, slot: BigInt(slot) };
}

/** The wallet's balance of `mint` in its associated token account; 0 if the account does not exist. */
export async function loadBalance(
  connection: Connection,
  owner: PublicKey,
  mint: PublicKey,
  tokenProgram: PublicKey
): Promise<bigint> {
  const address = getAssociatedTokenAddressSync(mint, owner, false, tokenProgram);
  try {
    const { value } = await connection.getTokenAccountBalance(address, 'confirmed');
    return BigInt(value.amount);
  } catch (error) {
    // A missing account is a zero balance; any other failure (a rate-limited RPC) must not look like one.
    if (error instanceof Error && /could not find account|Invalid param/i.test(error.message)) return 0n;
    throw error;
  }
}
