# Design notes

The technical findings that decide whether a hook design works. Everything here was either read
from the Token-2022 / transfer-hook-interface source or measured in the prototype this kit grew
out of (see "Where the numbers come from"). This is a note, not a manual: rules live in
`rule.rs`, behaviour in the tests.

## Execution lifecycle

1. A caller sends `TransferChecked` (or `TransferCheckedWithFee`) to Token-2022 for a mint that has
   the TransferHook extension.
2. Token-2022 validates and **moves the tokens**, sets the `transferring` flag on both token
   accounts, then calls the hook program with the SPL `Execute` instruction, and clears the flag.
3. The hook runs. If it returns an error, the whole transaction fails and every balance rolls back.

Consequences for rule authors:

* Balances read inside a hook are **post-transfer**.
* The `transferring` flag is the only proof the call came from Token-2022. A hook must check it on
  both accounts, or anyone can call `Execute` directly. The starter and `hook-kit` do.
* Token-2022 builds the `Execute` call with every account read-only and the authority as a
  non-signer. A hook cannot spend the transfer authority and cannot move the transferred tokens. It
  can sign for its own PDAs.
* Only transfers reach a hook. **Burns, mints, owner changes and delegate changes never do.**

## One hook program per mint

The extension stores exactly one hook program id. Selecting a hook is "set the mint's TransferHook
program"; it is permissionless: anyone can create a mint pointing at any program, including one that
refuses every transfer. Whether a *platform* accepts such a mint is that platform's policy, not a
property of the hook. Keep these separate:

* **Transport readiness**: the mint points at an existing, executable program with a well-formed
  validation list, so a transfer can reach it. Tooling can check this.
* **Business readiness**: the rule is sensible, the config is right, nobody can freeze transfers on
  purpose. Only a person reading the hook (or an audit) can say.

## ExtraAccountMetaList (the validation list)

A hook declares the extra accounts it needs in a PDA owned by the hook program, seeds
`["extra-account-metas", mint]`, holding an `ExtraAccountMetaList` (TLV, discriminator = `Execute`).
Each entry is a literal address, or a seeds-based PDA that can reference other accounts by index,
instruction data or account data, so it can be resolved from the mint and the transfer alone.

A wallet or integrator resolves them (`spl_token_2022::offchain`, or `@solana/spl-token`'s
`addExtraAccountMetasForExecute`) and appends them to the transfer instruction. The hook receives:

```text
source, mint, destination, authority,          // the transfer's own four
validation list,                               // the PDA above
N dynamic extras ...                           // from the list
```

and the transfer instruction carries, after its own accounts, **the N extras + the hook program +
the validation list**. The starter's list holds one seeds-based extra (its config PDA). Its bytes
are identical for every mint.

## Why callers must forward hook accounts

Token-2022 can only pass the hook accounts the *caller* put in the transaction. A program that
transfers a hooked token by CPI (an AMM, a vault, an escrow) must therefore accept the hook's
accounts from its own caller and forward them in its CPI. A program that does not forward them
cannot move that mint's tokens: the hook fails or is missing. This is why "supports Transfer Hooks"
is a property of the *calling* program. Whether a given Raydium program does is Raydium's to say;
check its docs. (The prototype showed it can be done without the AMM understanding any hook's
business logic. It only has to carry the accounts through. In that prototype the AMMs also required
the program admin to approve each Token-2022 mint with a hook before a pool could use it; that was
Raydium's existing rule and may have changed.)

## Atomicity on rejection

A rejection aborts the transaction, including whatever the transfer was part of: a swap, a deposit,
a bundle of instructions. This is the feature (the rule cannot be partly applied) and the risk (a
hook can block sells, withdrawals or migration if its authors want it to). Test every rejection for
its exact error code *and* for unchanged balances.

## Writable accounts and contention

Anything a hook writes must be declared writable in the validation list, and then **every transfer
of that mint** carries that account as writable. The runtime serialises transactions that write the
same account, and caps the compute any one account can consume per block. So a global counter
(fair-launch's slot counter, holder-rewards' global) is a throughput ceiling for the mint, and
spreading trades over several pools does not remove it. Prefer per-holder state where the rule
allows it; keep writes small. In the prototype, 16 simultaneous buys against a writable counter
all landed on a single local validator; that says nothing about a busy cluster.

Integrators should also refuse a writable or signer extra they did not expect. `hook-kit`'s test
helper `World::transfer_ix` does this: tests must name every writable extra on purpose.

## Compute, accounts and transaction size

Measured in-process against real binaries with a hook that only declares N extras (so a real rule
adds its own compute):

| What | Measure |
|---|---|
| A Token-2022 transfer through a hook with no extras | about 19,000 to 25,000 CU |
| Each PDA-derived extra account | roughly 13,000 to 15,000 CU |
| Practical ceiling on PDA-derived extras | about 10. The hook's 32 KiB heap ran out at 12 to 14, Token-2022's at 16+, before packet size or compute. A larger heap frame did not help |
| A legacy transaction stops fitting | about 24 to 32 extras (1,177 bytes at 24, 1,441 at 32); two hooked legs run out of room sooner |
| Address lookup tables | shrink the transaction (913 bytes to 298 at 16 extras) but do not lift the memory limit |
| Program rent | about 5.1 SOL per MB, refundable; a hook of this size is about 0.7 to 1 SOL |

`hook_kit::PRACTICAL_EXTRA_ACCOUNTS` (10) is a ceiling to stay well under, not a target. Literal
addresses are cheaper than PDA-derived extras and were not measured.

## Upgrade and config authority

Three different powers; never show them as one "owner":

| Power | What it can do | Where to read it |
|---|---|---|
| Program upgrade authority | replace the rule's code for **every** mint using the program | `solana program show <ID>`; revoke with `set-upgrade-authority --final` |
| Mint's TransferHook authority | re-point the mint at a different hook program | the mint's TransferHook extension (`None` once revoked) |
| Hook config authority | change one mint's parameters | the hook's config account; the starter has four modes (extension authority, mint authority, explicit, immutable) |

Every template README must say who holds each, and what a holder can do with it.

## Things a hook cannot do

* Run for burns or mints, or stop them. Revoke the mint authority if the rule depends on supply.
* Spend the transfer authority or move the transferred tokens.
* See a tip paid to a block builder, or the fee of a v1-format transaction (SIMD-0385 makes the
  compute-budget instructions no-ops there; a fee check based on them is bypassable).
* Know who a person is. Wallet and token-account rules are not person rules.
* Be trusted because it exists. Passing transport checks says nothing about business readiness.

## Where the numbers come from

They were measured in a prototype that ran arbitrary hooks through modified Raydium CPMM and CLMM
programs, with `solana-program-test` against SBF binaries (the real runtime, in-process; not a
validator, not a cluster under load) and a single local validator for contention. The prototype is
in git history (tag `pre-community-hook-kit`); its Raydium-specific parts are quarantined in
[`legacy/`](legacy). Nothing in this kit depends on it. Not measured: contention on a real
cluster, v1 transactions, literal-address extras, hostile hooks on a cluster.
