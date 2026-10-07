/** Read-only LaunchLab GlobalConfig inventory. Run with --help for usage. */
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import {
  DEVNET_PROGRAM_ID,
  LAUNCHPAD_PROGRAM,
  LaunchpadConfig,
  getPdaLaunchpadConfigId,
} from '@raydium-io/raydium-sdk-v2';
import { Connection, PublicKey, type AccountInfo } from '@solana/web3.js';
import bs58 from 'bs58';

export const accountDiscriminator = (name: string): Buffer =>
  createHash('sha256').update(`account:${name}`).digest().subarray(0, 8);

export function validateAccount(
  account: AccountInfo<Buffer> | null,
  programId: PublicKey,
  name: string,
  minimumLength = 8
): asserts account is AccountInfo<Buffer> {
  if (
    !account ||
    account.executable ||
    !account.owner.equals(programId) ||
    account.data.length < minimumLength ||
    !account.data.subarray(0, 8).equals(accountDiscriminator(name))
  ) {
    throw new Error(`Invalid ${name} account: owner, discriminator or data length mismatch`);
  }
}

export function decodeGlobalConfig(
  pubkey: PublicKey,
  account: AccountInfo<Buffer>,
  programId: PublicKey
) {
  validateAccount(account, programId, 'GlobalConfig', LaunchpadConfig.span);
  // Fail closed on layout changes instead of quietly omitting new fields.
  if (account.data.length !== LaunchpadConfig.span) {
    throw new Error(`Unsupported GlobalConfig size at ${pubkey.toBase58()}`);
  }
  const config = LaunchpadConfig.decode(account.data);
  const expected = getPdaLaunchpadConfigId(programId, config.mintB, config.curveType, config.index);
  if (!expected.publicKey.equals(pubkey)) throw new Error(`Invalid GlobalConfig PDA: ${pubkey}`);
  return {
    address: pubkey.toBase58(),
    epoch: config.epoch.toString(10),
    curveType: config.curveType,
    index: config.index,
    mintB: config.mintB.toBase58(),
    migrateFee: config.migrateFee.toString(10),
    tradeFeeRate: config.tradeFeeRate.toString(10),
    maxShareFeeRate: config.maxShareFeeRate.toString(10),
    minSupplyA: config.minSupplyA.toString(10),
    maxLockRate: config.maxLockRate.toString(10),
    minSellRateA: config.minSellRateA.toString(10),
    minMigrateRateA: config.minMigrateRateA.toString(10),
    minFundRaisingB: config.minFundRaisingB.toString(10),
    protocolFeeOwner: config.protocolFeeOwner.toBase58(),
    migrateFeeOwner: config.migrateFeeOwner.toBase58(),
    migrateToAmmWallet: config.migrateToAmmWallet.toBase58(),
    migrateToCpmmWallet: config.migrateToCpmmWallet.toBase58(),
    // Preserve reserved/new fields that the installed SDK does not decode.
    dataBase64: account.data.toString('base64'),
  };
}

export async function fetchGlobalConfigs(
  connection: Pick<Connection, 'getProgramAccounts'>,
  programId: PublicKey
) {
  // No mint, curve/index, or size filter: enumerate every GlobalConfig and reject
  // unknown layouts, rather than silently returning an incomplete inventory.
  const response = await connection.getProgramAccounts(programId, {
    commitment: 'confirmed',
    withContext: true,
    filters: [{ memcmp: { offset: 0, bytes: bs58.encode(accountDiscriminator('GlobalConfig')) } }],
  });
  const configs = response.value.map(({ pubkey, account }) =>
    decodeGlobalConfig(pubkey, account, programId)
  );
  configs.sort((a, b) => (a.address < b.address ? -1 : a.address > b.address ? 1 : 0));
  if (new Set(configs.map((config) => config.address)).size !== configs.length) {
    throw new Error('RPC returned duplicate GlobalConfig accounts');
  }
  return { slot: response.context.slot, configs };
}

export const commonOptions = {
  'rpc-url': { type: 'string' },
  cluster: { type: 'string', default: 'devnet' },
  help: { type: 'boolean', default: false },
} as const;

export async function connect(values: { 'rpc-url'?: string; cluster?: string }) {
  const cluster = values.cluster ?? 'devnet';
  if (cluster !== 'devnet' && cluster !== 'mainnet-beta') {
    throw new Error('--cluster must be devnet or mainnet-beta');
  }
  const rpcUrl = values['rpc-url'] ?? process.env.RPC_URL;
  if (!rpcUrl)
    throw new Error('Provide --rpc-url or RPC_URL (no env file is loaded automatically)');
  const connection = new Connection(rpcUrl, 'confirmed');
  const genesisHash = await connection.getGenesisHash();
  // getGenesisHash returns the full hash, not the shortened CAIP-2 chain ID.
  // https://github.com/solana-labs/solana/blob/master/sdk/src/genesis_config.rs
  const expected =
    cluster === 'devnet'
      ? 'EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG'
      : '5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d';
  if (genesisHash !== expected)
    throw new Error(
      `RPC genesis hash does not match --cluster ${cluster}: expected ${expected}, received ${genesisHash}`
    );
  const programId = cluster === 'devnet' ? DEVNET_PROGRAM_ID.LAUNCHPAD_PROGRAM : LAUNCHPAD_PROGRAM;
  return { connection, cluster, programId, genesisHash };
}

export function isMain(url: string): boolean {
  return !!process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === url;
}

export function reportError(error: unknown): void {
  // RPC errors can include URLs with API keys. Never echo credentials in CLI logs.
  const message = error instanceof Error ? error.message : 'Unknown error';
  console.error(message.replace(/https?:\/\/[^\s"'<>]+/g, '[RPC URL redacted]'));
  process.exitCode = 1;
}

async function main() {
  const { values } = parseArgs({ options: commonOptions, strict: true });
  if (values.help) {
    console.log(`Usage: node --import tsx scripts/fetch-launchlab-global-configs.ts
  --rpc-url <url>                  Or RPC_URL environment variable
  --cluster <devnet|mainnet-beta>  Default: devnet; checked against RPC genesis hash

Writes a JSON inventory to stdout. Requires complete getProgramAccounts support.
All u64 values are decimal strings. No signer, env-file loading or chain writes.`);
    return;
  }
  const { connection, programId, cluster, genesisHash } = await connect(values);
  const inventory = await fetchGlobalConfigs(connection, programId);
  console.log(
    JSON.stringify(
      { schemaVersion: 1, cluster, genesisHash, programId: programId.toBase58(), ...inventory },
      null,
      2
    )
  );
}

if (isMain(import.meta.url)) main().catch(reportError);
