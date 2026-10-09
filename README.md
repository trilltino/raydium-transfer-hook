# Community Transfer Hook Kit

A minimal, AI-friendly starter kit for designing, testing and deploying Token-2022 Transfer Hooks,
including hooks meant to be used with Raydium pools.

**Status:** independent community project, maintained in a personal capacity. **Not affiliated with,
endorsed by or maintained by Raydium**, not an official Raydium release, and not audited. See the
[Disclaimer](#disclaimer). Check [Raydium's canonical documentation](#8-canonical-solana-and-raydium-resources)
for current deployed Transfer Hook support.

You have an idea for token business logic. You start in one file, `rule.rs`, test it, deploy it to
devnet and, if it is good, contribute it back as a template.

```text
UNDERSTAND  ->  CREATE  ->  TEST  ->  DEPLOY  ->  CONTRIBUTE
 README         rule.rs     cargo test  scripts/     templates/ PR
 AGENTS.md                              deploy.sh
```

## 1. What is this?

* [`starter/`](starter): a deployable hook with the plumbing done. Its rule is one line:
  *reject a transfer above a limit*. You replace the rule.
* [`templates/`](templates): three reference templates with richer designs (launch controls,
  vesting, holder rewards), each with the list of what it cannot do.
* [`hook-kit/`](hook-kit): the shared plumbing the templates use (Execute checks, mint and token
  reads, PDA creation, in-process test helpers).
* [`scripts/deploy.sh`](scripts/deploy.sh): build and deploy any hook in one command, and prove it on
  the cluster when the hook ships a setup example (the starter does).
* [`AGENTS.md`](AGENTS.md): the contract for AI coding agents working in this repository.
* [`DESIGN.md`](DESIGN.md): the few technical facts about hooks that decide whether a design works.

It is **not** a Raydium distribution. It contains no Raydium code, no Raydium program ids and no
Raydium forks.

## 2. What is a Transfer Hook?

A Token-2022 mint can name one *hook program*. On every transfer of that mint, Token-2022 calls the
hook (the SPL `Execute` instruction) **after** moving the tokens. If the hook returns an error, the
whole transaction fails and every balance rolls back. So a hook can enforce a rule on every transfer,
whichever program started it: a wallet, a swap, a vault.

`Execute` receives the transfer's accounts read-only and without the transfer authority's
signature, so a hook cannot re-spend the transferred tokens through that authority (it can still
sign for its own PDAs). It never sees burns, mints or owner changes, because they are not
transfers. It declares the extra accounts it needs in an `ExtraAccountMetaList` that callers must
forward. [`DESIGN.md`](DESIGN.md) has the details that matter for design.

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
(cd my-hook && cargo test --locked)   # 3. test it: allowed, boundary, rejected, config, authority
scripts/deploy.sh my-hook             # 4. build, deploy to devnet, then set up a mint and prove the rule
```

**Start in `rule.rs`.** Simple rules that fit the existing config (one `u64` today, up to 256 bytes
of params) may only need that file. Rules that introduce new configuration, state or extra accounts
must also update the corresponding config/account plumbing and tests; [`starter/README.md`](starter/README.md)
says which files.

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
test only_the_live_hook_authority_can_initialize ... ok
test second_initialize_fails_with_already_initialized_and_changes_nothing ... ok
```

### What `scripts/deploy.sh` does

Devnet and `~/.config/solana/id.json` by default; `scripts/deploy.sh --help` lists the options, and
[`scripts/README.md`](scripts/README.md) has the details. Mainnet is refused without `--allow-mainnet`.

* **Every hook:** checks the tools and keypair, builds the SBF program and deploys it under the
  program id `cargo build-sbf` generated.
* **Hooks with `examples/devnet.rs` (the starter and Creator Commitment):** also creates a Token-2022 mint whose Transfer
  Hook points at your program, initialises the hook (config and validation list), and sends one
  transfer that must pass and one that must be refused. Example options go after `--`:
  `scripts/deploy.sh my-hook -- --limit 1000`.

```text
PASS  transfer of 500 (= limit) allowed
PASS  transfer of 501 (limit + 1) refused by the hook (custom error 0x700b)
cluster            https://api.devnet.solana.com
hook program id    <YOUR_PROGRAM_ID>
mint               <NEW_MINT_ADDRESS>
hook config PDA    <CONFIG_PDA>
validation list    <VALIDATION_LIST_PDA>
```

The addresses are new on every run. Read them before you trust the hook: the deployer is still the
program's **upgrade authority** and can replace your rule for every mint. Revoke it with
`solana program set-upgrade-authority <PROGRAM_ID> --final`, or say publicly who holds it.

Fair Launch and Holder Rewards have no `examples/devnet.rs` yet: the script deploys them and stops.
Each template README's `DEPLOY / INITIALIZE` section says what its initialise instruction needs, and
its tests show a working call.

## 5. Included templates

| Template | Maturity | One line | Rule file |
|---|---|---|---|
| [Fair Launch](templates/fair-launch) | reference | Launch participation controls (basic bundle and snipe resistance): caps on buy size, balance and buys per slot, and a priority-fee limit, inside a launch window | [`rule.rs`](templates/fair-launch/src/rule.rs) |
| [Creator Commitment](templates/creator-commitment) | stable | A creator account cannot fall below its current vesting floor; everything above it moves freely | [`rule.rs`](templates/creator-commitment/src/rule.rs) |
| [Holder Rewards](templates/holder-rewards) | experimental | Holders earn a reward token by balance x time, using a global reward index (no loop over holders) | [`rule.rs`](templates/holder-rewards/src/rule.rs) |

Maturity labels are defined in [`templates/README.md`](templates/README.md); none means audited.
Every template README answers the same questions: what, why, trigger, example, rules, **limitations**,
**trust**, state and cost, tests, and how to deploy and initialise it. A template that cannot say
what it fails to stop is not accepted.

## 6. Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md). A template pull request has a fixed checklist (problem
statement, `rule.rs`, positive, rejection, boundary, config and authority tests, bypasses, accounts,
writable accounts, authority and upgrade assumptions, and the evidence level reached). This
repository curates templates; it does not accept every working piece of Rust. AI agents: read
[`AGENTS.md`](AGENTS.md) first.

## 7. Security / disclaimer

A hook is an untrusted program that can refuse any transfer, and a refusal aborts the whole
transaction. Nothing here is audited. Hooks in this repository are reference implementations and
teaching material, not endorsed or safe-by-construction products. Three separate powers can change
what a token does: the hook program's **upgrade authority** (replaces the code for every mint), the
mint's **Transfer Hook authority** (points the mint at another hook), and the hook's **config
authority** (sets one mint's parameters; set once in this kit). Minting and burning are not
transfers, so revoke the mint authority if your rule depends on supply. Do not deploy to mainnet
without independent review.

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
`pre-community-hook-kit`), including the Raydium-specific adapters, SDK and TypeScript client. It is
not part of this kit.

## Disclaimer

* This software is provided **"as is", without warranty of any kind**, under the [MIT License](LICENSE).
  The authors and contributors are not liable for any claim, damages or loss arising from its use.
* It is **not audited** and is not financial, investment or legal advice. Nothing here recommends
  deploying a program or creating, buying or selling any token.
* **You are solely responsible** for any program you deploy from it, for who holds its upgrade,
  Transfer Hook and config authorities, for the keys you use, and for complying with the laws that
  apply to you and your token.
* This is an independent project maintained in a personal capacity. It does not represent Raydium
  or any employer. "Raydium" names a third-party protocol; no affiliation or endorsement is implied.
* To report a vulnerability, see [`SECURITY.md`](SECURITY.md).

## License

[MIT](LICENSE).
