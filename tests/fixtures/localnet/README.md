# Throwaway localnet keys

**Public on purpose. Never fund these on any public cluster.**

These keypairs exist so a clean checkout can run every Raydium + Transfer Hook flow with no
private key (`cargo xtask localnet build | validator | e2e`, and the in-process tests):

| File | Is |
|---|---|
| `admin.json` | The admin baked into the `localnet` builds of the hook-support forks (passed at build time as `CPSWAP_LOCALNET_ADMIN` / `CLMM_LOCALNET_ADMIN`), the faucet of the local validator, and the payer of every flow |
| `reference-hook.json`, `arbitrary-hook.json`, `creator-commitment.json`, `fair-launch.json`, `holder-rewards.json` | Program ids of the hooks on localnet (`environments/localnet.json`) |

The Raydium programs keep upstream's program ids on localnet, and the CPMM pool-creation fee
receiver (upstream's address, whose key nobody here holds) is seeded at genesis as an empty
wrapped-SOL account, so no key is needed for either.

Devnet uses different keys, which are never committed (`.keys/`, see `docs/forking.md`).
