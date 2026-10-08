# Scripts

## `approve-hooked-mints.ts`: approve hooked mints for pool creation

Raydium's CPMM and CLMM admit a Token-2022 mint with a TransferHook extension to a new pool only if
the program's admin has approved that mint (a `SupportMintAssociated` record). The programs accept
exactly two signers for the approval: their compile-time admin and one fixed owner key. So **whoever
deploys the forks with their own keys approves the mints**; nobody else can.

This script does that job (and checks whether a mint is approved) with `@solana/web3.js` and
`@solana/spl-token` only. It needs no Raydium SDK: the instruction is an 8-byte discriminator and four
accounts. It reads the same environment files as the Rust CLI (`environments/*.json`), and does the same
thing as `raydium-hook mint approve` / `mint approval`; use whichever you prefer.

```sh
cd scripts
npm install                     # Node 22 or later

# is it approved? (read-only, no key)
npm run status -- --env ../environments/mine.json --mint MINT

# approve one mint, several, or a file with one address per line (# comments allowed)
npm run approve -- --env ../environments/mine.json --keypair ../.keys/deployer.json --mint MINT
npm run approve -- --env ../environments/mine.json --keypair ../.keys/deployer.json --mints-file mints.txt

# simulate everything, send nothing
npm run approve -- ... --dry-run
```

What it does, in order: refuses a key that is not the admin the environment records (and says which key
the program expects); checks each mint (a Token-2022 mint with a TransferHook extension, and if a hook
program is set, that it exists and is executable); skips mints already approved; packs many approvals
into as few transactions as fit; simulates each transaction before sending; exits with status 1 if any
approval did not go through. `--amm cpmm|clmm|all` (default `all`) chooses the programs.

A mint whose hook is not set yet is approvable on purpose: the usual order is to create the mint with
the extension, approve it, create the pool, then attach the hook.

### Tests and checks

```sh
npm test            # unit tests: instruction bytes, record checks, packing, mint checks
npm run typecheck
```

It has also been run against the devnet deployment recorded in `environments/devnet.json`: a status
check (not approved), a dry run (simulation passed, nothing changed), the real approval on both
programs, a second run (skipped as already approved), a key that is not the admin (refused), and an
address that is not a Token-2022 mint (refused).

### Known differences from the Rust command

* The Rust command also checks the hook's validation list and reports a problem as a note; this script
  does not.
* Neither pins the cluster by genesis hash. Program ids differ per cluster, so a wrong RPC URL finds no
  program rather than approving something on the wrong cluster.

### Dependencies

Versions are pinned exactly (`package.json`, `package-lock.json`). `npm audit` reports nine advisories
(three high) in the transitive dependencies of `@solana/web3.js` 1.x (`bigint-buffer`, `uuid`,
`jayson`, `stream-json`); the fix is `@solana/web3.js` 3, a breaking change that this script does not
take. The script reads a local keypair file and responses from the RPC you configured, and parses no
other input; run it against an RPC you trust.
