# Community Transfer Hook Kit

A small starter kit for building, testing and deploying Token-2022 Transfer Hooks, including hooks intended for use with Raydium pools.

This is an **independent community project**, not an official Raydium release and not audited. Check Raydium's canonical documentation for the current state of deployed Transfer Hook support.

```text
idea
  ↓
copy starter
  ↓
edit rule.rs
  ↓
test
  ↓
deploy to devnet
  ↓
contribute a template
```

## What this is

- [`starter/`](starter) — a standalone hook you can copy and modify. Start in `src/rule.rs`.
- [`templates/`](templates) — reference hooks showing more complete designs and their trade-offs.
- [`hook-kit/`](hook-kit) — shared Transfer Hook plumbing used by the templates.
- [`scripts/deploy.sh`](scripts/deploy.sh) — builds and deploys hooks, with optional devnet setup examples.
- [`AGENTS.md`](AGENTS.md) — implementation and security guidance for AI coding agents.
- [`DESIGN.md`](DESIGN.md) — the Transfer Hook internals that matter when designing a rule.

This repository does **not** contain Raydium program forks and is not a replacement for Raydium's SDK or program repositories.

## Transfer Hooks in 30 seconds

A Token-2022 mint can point to a Transfer Hook program.

During a transfer:

```text
TransferChecked
      ↓
   Token-2022
      ↓
Transfer Hook Execute
      ↓
    your rule
      ↓
 allow / reject
```

Token-2022 invokes the hook as part of the transfer. If the hook returns an error, the transaction fails and the transfer rolls back.

A hook can inspect the transfer and any extra accounts declared through its `ExtraAccountMetaList`. This makes it useful for rules such as launch controls, vesting restrictions and stateful accounting.

Hooks run on **transfers**. Minting, burning and token-account owner changes are separate operations and must be considered explicitly when designing a rule.

See [`DESIGN.md`](DESIGN.md) for the deeper execution and account model.

## Quickstart

Requirements:

- Rust
- Solana CLI / `cargo build-sbf`
- a funded devnet keypair for deployment

```sh
git clone https://github.com/trilltino/raydium-transfer-hook
cd raydium-transfer-hook

cp -r starter my-hook

$EDITOR my-hook/src/rule.rs

(cd my-hook && cargo test --locked)

scripts/deploy.sh my-hook
```

The starter begins with a simple rule:

```rust
pub fn check_transfer(
    config: &HookConfig,
    context: &TransferContext,
) -> Result<(), HookError> {
    if context.amount > config.max_transfer_limit()? {
        return Err(HookError::TransferExceedsLimit);
    }

    Ok(())
}
```

**Start in `rule.rs`.**

A simple rule that fits the starter's existing configuration may only require changing that file. A hook that introduces additional state, configuration or extra accounts will also need the corresponding account and instruction plumbing.

The starter tests cover allowed transfers, exact boundaries, rejection, configuration validation, authority checks and direct `Execute` attempts.

## Reference templates

| Template | Maturity | Purpose | Main trade-off |
|---|---|---|---|
| [`creator-commitment`](templates/creator-commitment) | stable | Keeps one committed token account above a time-dependent vesting floor | Account-level commitment; burns are not transfers |
| [`fair-launch`](templates/fair-launch) | reference | Launch controls for buy size, account balance, buys per slot and optional priority-fee limits | Basic snipe/bundle resistance, not identity or complete bundle detection |
| [`holder-rewards`](templates/holder-rewards) | experimental | Balance × time reward accounting using a global index | Explicit registration, additional state and global writable-account contention |

Each template documents:

- what the hook is trying to solve
- which transfers trigger it
- its rule and expected behaviour
- known bypasses and limitations
- required state and accounts
- writable-account / contention costs
- authority and upgrade assumptions
- tests and deployment status

Read a template's **LIMITATIONS** and **TRUST** sections before using it.

More possible hook designs are collected in [`templates/IDEAS.md`](templates/IDEAS.md).

## Deploying to devnet

The deployment helper is:

```sh
scripts/deploy.sh <HOOK_DIR>
```

For example:

```sh
scripts/deploy.sh starter
```

or with hook-specific example arguments:

```sh
scripts/deploy.sh starter -- --limit 1000
```

The script:

1. builds the program with `cargo build-sbf`;
2. deploys it to the selected cluster;
3. if the hook contains `examples/devnet.rs`, runs that setup example.

Devnet is the default.

Mainnet deployment is refused unless it is explicitly enabled with `--allow-mainnet`.

### Current deployment evidence

The **starter** and **Creator Commitment** ship devnet examples. They can:

- deploy the hook;
- create a fresh Token-2022 mint;
- configure the Transfer Hook;
- initialise the hook's state and validation list;
- execute an allowed transfer;
- confirm the expected rejection against devnet RPC simulation.

Fair Launch and Holder Rewards currently have in-process and SBF runtime tests but do not yet ship full devnet setup examples.

See [`scripts/README.md`](scripts/README.md) and each template's `DEPLOY / INITIALIZE` section for the exact flow.

## How the shared plumbing works

Templates use [`hook-kit`](hook-kit) so their business logic does not need to reimplement the Transfer Hook lifecycle.

Before template logic runs, the shared execution path checks things such as:

- valid `Execute` instruction data;
- expected account count;
- Token-2022 mint and token accounts;
- the mint points at the current hook program;
- the call is happening during a real Token-2022 transfer;
- the expected validation-list PDA is present;
- the validation list matches the canonical account shape expected by the template.

The template can then focus on its own rule.

For example:

```text
Token-2022
    ↓
execute_prelude()
    ↓
template processor
    ↓
rule.rs
```

The starter carries similar plumbing directly so it remains standalone and can be copied out of this repository.

## Contributing

This repository curates reference hooks rather than accepting every working piece of Rust.

A template should make it easy to answer:

- What problem does this solve?
- Why is a Transfer Hook the right place to solve it?
- Which transfers trigger the rule?
- Which transfers do not?
- What state and extra accounts are required?
- Which accounts are writable?
- What contention or compute cost does it introduce?
- Who controls its configuration?
- Can the program or hook selection change?
- How can the rule be bypassed?
- What evidence level has actually been reached?

Tests should include valid behaviour, rejection, exact boundaries, malformed configuration, authority checks, irrelevant transfer paths and state correctness.

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full checklist.

If you are building with an AI coding agent, have it read [`AGENTS.md`](AGENTS.md) before modifying the hook.

## Raydium integration

This repository builds **Token-2022 Transfer Hooks**.

Raydium program and SDK support is a separate integration layer. Applications should use Raydium's canonical programs, SDK and documentation for the current production interface and account requirements.

- Raydium docs: <https://docs.raydium.io>
- Raydium GitHub: <https://github.com/raydium-io>
- Solana Transfer Hook guide: <https://solana.com/developers/guides/token-extensions/transfer-hook>
- SPL Transfer Hook interface: <https://github.com/solana-program/transfer-hook>
- Token-2022: <https://github.com/solana-program/token-2022>

This repository originally contained a larger research prototype that exercised hooks through modified CPMM and CLMM programs. That work is preserved in git under the `pre-community-hook-kit` tag; it is not part of the current starter kit.

## Security and trust

Transfer Hooks are part of a token's transfer path. A bug or malicious rule can prevent transfers.

Nothing in this repository is audited.

When evaluating a hook, keep the different authorities separate:

- **program upgrade authority** — can replace the hook program's code;
- **Transfer Hook authority** — can change which hook program a mint uses;
- **hook configuration authority** — may control a particular hook's parameters.

Some templates intentionally make configuration immutable after setup, but program and mint authorities can still matter.

Hooks also do not automatically observe minting, burning or token-account ownership changes.

Do not deploy a reference template to mainnet without reviewing its implementation, tests, authority model and documented limitations.

See [`SECURITY.md`](SECURITY.md) for vulnerability reporting.

## License

[MIT](LICENSE)
