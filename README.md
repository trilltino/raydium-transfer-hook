# Raydium Transfer Hook Kit

A minimal, AI-friendly starter kit for designing, testing and deploying Token-2022 Transfer Hooks
that can be used with Raydium.

You have an idea for token business logic. You write it in one file, `rule.rs`. You test it, deploy
it to devnet, and, if it is good, contribute it back as a template.

```text
UNDERSTAND  ->  CREATE  ->  TEST  ->  DEPLOY  ->  CONTRIBUTE
 README         rule.rs     cargo test  scripts/     templates/ PR
 AGENTS.md                              deploy.sh
```

## 1. What is this?

* [`starter/`](starter): a deployable hook with all the plumbing done. Its rule is one line:
  *reject a transfer above a limit*. You replace the rule.
* [`templates/`](templates): three reviewed hooks that show richer designs (launch controls, vesting,
  holder rewards), each with the list of what it cannot do.
* [`hook-kit/`](hook-kit): the shared plumbing the templates use (Execute checks, mint and token
  reads, PDA creation, in-process test helpers).
* [`scripts/`](scripts): `build.sh`, `test.sh`, `deploy.sh`.
* [`AGENTS.md`](AGENTS.md): the contract for AI coding agents working in this repository.
* [`DESIGN.md`](DESIGN.md): the few technical facts about hooks that decide whether a design works.

It is **not** a Raydium distribution. It contains no Raydium code, no Raydium program ids and no
Raydium forks. See [section 8](#8-canonical-solana-and-raydium-resources).

## 2. What is a Transfer Hook?

A Token-2022 mint can name one *hook program*. On every transfer of that mint, Token-2022 calls the
hook (the SPL `Execute` instruction) **after** moving the tokens. If the hook returns an error, the
whole transaction fails and every balance rolls back. So a hook can enforce a rule on every transfer,
whichever program started it: a wallet, a swap, a vault.

A hook cannot move the tokens, cannot see burns or owner changes (they are not transfers), and
declares the extra accounts it needs in an `ExtraAccountMetaList` that callers must forward.
[`DESIGN.md`](DESIGN.md) has the details that matter for design.

## 3. What can I build?

Anything that is a yes/no decision on a transfer, optionally with a little state:

| Class | Example | Template |
|---|---|---|
| Allow / reject policy | launch caps, max transfer, allowlists | [`fair-launch`](templates/fair-launch), [`starter`](starter) |
| Time-dependent restriction | vesting a creator allocation | [`creator-commitment`](templates/creator-commitment) |
| Stateful economic accounting | pay holders by balance x time | [`holder-rewards`](templates/holder-rewards) |

More ideas, with why most of them are not templates yet: [`templates/IDEAS.md`](templates/IDEAS.md).

## 4. Quickstart

Needs Rust and the [Solana CLI tools](https://solana.com/docs/intro/installation) (`cargo build-sbf`).
On Windows, run the scripts from Git Bash.

```sh
git clone https://github.com/trilltino/raydium-transfer-hook && cd raydium-transfer-hook

cp -r starter my-hook                 # 1. copy the starter (renaming the crate is optional, see starter/README.md)
$EDITOR my-hook/src/rule.rs           # 2. change the rule (find "YOUR BUSINESS LOGIC HERE")
(cd my-hook && cargo test)            # 3. test it: allowed, boundary, rejected, config, authority
scripts/build.sh my-hook              # 4. build it for Solana (SBF)
scripts/deploy.sh my-hook             # 5. deploy to devnet and configure a mint (below)
```

The rule is this, in [`starter/src/rule.rs`](starter/src/rule.rs):

```rust
pub fn check_transfer(config: &HookConfig, context: &TransferContext) -> Result<(), HookError> {
    // YOUR BUSINESS LOGIC HERE. Inclusive: amount == limit passes.
    if context.amount > config.max_transfer_limit()? {
        return Err(HookError::TransferExceedsLimit);
    }
    Ok(())
}
```

`cargo test` in the starter prints, among others:

```text
test rule::tests::a_transfer_below_the_limit_is_allowed ... ok
test rule::tests::a_transfer_exactly_at_the_limit_is_allowed ... ok
test rule::tests::a_transfer_one_over_the_limit_is_rejected ... ok
test rule::tests::a_zero_limit_or_malformed_params_are_not_a_valid_configuration ... ok
test initialize_enforces_the_authority_required_by_each_mode ... ok
test update_config_requires_current_seq_and_the_mode_authority ... ok
```

### What `scripts/deploy.sh` does

Defaults to devnet and `~/.config/solana/id.json`; see `scripts/deploy.sh` for the options.

1. Checks the Solana tools and the deployer keypair, picks the cluster, tops up from the devnet faucet.
2. Builds the SBF program and deploys it under the program id `cargo build-sbf` generated.
3. Runs `my-hook/examples/devnet.rs`: creates a Token-2022 mint with the Transfer Hook extension
   pointing at your program, initialises the hook for that mint (config and validation list), then
   sends one transfer that must pass and one that must be refused.
4. Prints the cluster, hook program id, mint, config and validation-list addresses, and the next step.

```text
PASS  transfer of 500 (= limit) allowed
PASS  transfer of 501 (limit + 1) refused by the hook (custom error 0x700b)
cluster            https://api.devnet.solana.com
hook program id    71vxfXYNTm2yUwWLGpVT6zSRhRkXuQhwPkTL2FJ9sNCp
mint               9TV3tKWbbdChXGJyL9bJthypKabBAxjcn5vAbaKqqwGf
hook config PDA    FsP6xViFdUjnUsKKpD7yfaFaZM6GN99bz67HgZiSWS9h
validation list    5UwNi3pEpPokoKckfsZ47P6uYXc1ssveNLAJ9EWV1E91
```

That is the output of a real run of the unmodified starter on devnet. Read it before you trust it:
the deployer is still the program's **upgrade authority** and can replace your rule for every mint.
Revoke it with `solana program set-upgrade-authority <PROGRAM_ID> --final`, or say publicly who holds it.

`cargo build-sbf` may print `Function ... overflows the maximum allowed frame space` lines for crypto
crates inside the dependencies. They are not in code paths the hook runs; the build still succeeds.

Templates take their own setup parameters (a launch window, a vesting schedule, a reward vault). Each
template README says what its `Initialize` instruction needs, and its tests show a working call.

## 5. Included templates

| Template | One line | Rule file |
|---|---|---|
| [Fair Launch](templates/fair-launch) | Basic bundle and snipe resistance: caps on buy size, balance and buys per slot, and a priority-fee limit, inside a launch window | [`rule.rs`](templates/fair-launch/src/rule.rs) |
| [Creator Commitment](templates/creator-commitment) | A creator account cannot fall below its current vesting floor; everything above it moves freely | [`rule.rs`](templates/creator-commitment/src/rule.rs) |
| [Holder Rewards](templates/holder-rewards) | Holders earn a reward token by balance x time, using a global reward index (no loop over holders) | [`rule.rs`](templates/holder-rewards/src/rule.rs) |

Every template README answers the same questions: what, why, trigger, example, rules, **limitations**,
**trust**, state and cost, tests. A template that cannot say what it fails to stop is not accepted.

## 6. Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md). A template pull request has a fixed checklist
(problem statement, `rule.rs`, positive, rejection and boundary tests, bypasses, accounts, writable
accounts, authority and upgrade assumptions, a deployment run). This repository curates templates; it
does not accept every working piece of Rust. AI agents: read [`AGENTS.md`](AGENTS.md) first.

## 7. Security / disclaimer

A hook is an untrusted program that can refuse any transfer. Nothing here is audited. Hooks in this
repository are examples and teaching material, not endorsed or safe-by-construction products. Whoever
holds a hook program's upgrade authority, or a mint's Transfer Hook authority, can change what the
hook does. Do not deploy to mainnet without your own review.

## 8. Canonical Solana and Raydium resources

This repository does not own or maintain Raydium's programs or SDK, and it does not assert what
Raydium's deployed programs currently accept. Use Raydium's own documentation and SDK for pool
integration, and check them for the current state of Transfer Hook support before you rely on it.

* Raydium: <https://docs.raydium.io> and <https://github.com/raydium-io>
* Token-2022 Transfer Hook guide: <https://solana.com/developers/guides/token-extensions/transfer-hook>
* SPL Transfer Hook interface: <https://github.com/solana-program/transfer-hook>
* Token-2022 program: <https://github.com/solana-program/token-2022>
* Solana CLI install: <https://solana.com/docs/intro/installation>

History: this repository began as a prototype that ran arbitrary hooks through modified Raydium CPMM
and CLMM programs, to show the architecture works. That prototype is preserved in git (tag
`pre-community-hook-kit`) and, in part, in [`legacy/`](legacy). It is not part of this kit.
