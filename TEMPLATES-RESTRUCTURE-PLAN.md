# Plan: a lean reference repo, with one complete starter and three examples

Status: proposed. Part of Phase 1 is already done on `main`; everything else is not started.

| Done on `main` | Commit |
|---|---|
| Removed `reference-hook-model`, `hook-template-sdk`, `integrations/launchlab`, `tests/models` and the CLI `template` command | `20c2a2b` |
| Removed `programs/hook-template-registry` and `integrations/{cpmm,clmm}` | `730c6ff` |
| Docs: removed `permissionless-hooks.md`; trimmed `source-lock.md` and `security.md`; fixed the index | `335f0e4`, `d807d0e` |

Phase 1 is complete: the `xtask` devnet tooling is kept (D6 reversed, see the table). The workspace is at
18 members today (the plan's target is 15, plus the starter, which the merges below reach).

## Goal

A developer forks this repository to write a Transfer Hook. They should find:

* **One starter** that already contains every piece of hook plumbing, as optional modules.
* **Three examples built on it**, each reduced to its own rule (plus tests), so what is specific to
  the example is obvious and the rest is visibly shared.
* **Nothing else they do not need.** Code that nothing in the real stack uses is removed, and
  duplicates are merged.
* Honest documentation of the limits, including the commercial side.

## Where the repository is today

Measured on the working tree (Rust lines including tests; 25 workspace members, plus the starter,
which sits outside the workspace):

| Group | Lines | Used by the real stack (driver, CLI, `xtask`, flows)? |
|---|---|---|
| `transfer-hook-sdk`, `hook-policy-model`, `hook-kit` | ~7,100 | yes: the core |
| `raydium-adapters`, `raydium-hook-driver`, `raydium-hook-cli` | ~8,100 | yes: run and prove everything |
| `xtask` | 1,282 | `localnet` and `upstream` (~790): yes. `devnet` and `devnet_doc` (~495): maintainers only (they need the `.keys/` files) |
| the starter and five examples | ~8,900 | yes |
| `programs/reference-hook-onchain` | 5,311 | yes: the driver depends on it. Near-duplicate of the starter's layout |
| `programs/arbitrary-test-hook` | 1,044 | yes: proves there is no allowlist |
| `benches/harness`, `programs/bench-hook` | ~1,460 | benchmarks only; they back the limit numbers in the docs |
| `programs/hook-template-registry`, `crates/hook-template-sdk` | ~1,140 | only the CLI's `template` command |
| `integrations/{cpmm,clmm,launchlab}`, `crates/reference-hook-model`, `tests/models` | ~1,720 | **no**: they only use each other; the README already says not to treat their numbers as measurements |

`anti-bundle` is a strict subset of `fair-launch` (its per-slot budget is one of fair-launch's four
rules), `parent-spin-off` is `loyalty-rewards` plus "fund once", and each example carries its own
copy of the plumbing.

## Target shape

```text
crates/
  transfer-hook-sdk/        resolve a leg's hook accounts, frame the Raydium instructions
  hook-policy-model/        (kept: the SDK depends on it)
  hook-kit/                 shared on-chain helpers and the pure transfer-context types
  raydium-adapters/         instruction builders for CPMM and CLMM
  raydium-hook-driver/      the end-to-end flows
  raydium-hook-cli/         `raydium-hook` (the `template` command is gone)
templates/
  transfer-hook-starter/    the complete plumbing, modules you can switch on, one rule.rs to edit;
                            its default rule is the old reference hook's max-transfer rule
  fair-launch/              rule: buy caps, per-wallet cap, per-slot budget, priority fee
  creator-commitment/       rule: vesting floor on one dedicated account
  holder-rewards/           rule: balance x time; modes: ongoing (top-ups) and one-time (spin-off)
programs/
  arbitrary-test-hook/      a hook that shares nothing with the starter (the no-allowlist proof)
  bench-hook/               measurement only
benches/                    harness and results
tests/                      program-test (the in-process flows), third-party-hook
xtask/                      localnet and upstream only
```

Before and after:

| | Before | After |
|---|---|---|
| Workspace members | 25 (+ the starter) | 15 (+ the starter) |
| Rust lines | ~36,700 | ~26,600, plus a few hundred for the new starter modules |

The numbers are an estimate from the table above; Phase 9 records the real ones.

`anti-bundle` and `parent-spin-off` do not vanish from the docs: they become named configurations
(`max_buys_per_slot` only; `mode = one-time`) of their parent example.

## Decisions needed before Phase 1 (the plan assumes the recommended answer)

| # | Question | Recommended |
|---|---|---|
| D1 | Delete the `anti-bundle` and `parent-spin-off` crates, or keep them as thin wrappers? | Delete; keep their old devnet entries in `environments/devnet.json` as history, marked superseded. |
| D2 | Keep program ids of the merged examples? | New ids for the merged programs; the old ids stay deployed and untouched. |
| D3 | Where do shared modules live: in the starter or in `hook-kit`? | Pure logic with no Solana types goes in `hook-kit`; anything that needs the starter's config and PDA layout goes in the starter. Examples depend on `hook-kit` and copy nothing. |
| D4 | Delete the model island (`integrations/*`, `reference-hook-model`, `tests/models`)? | Delete. Nothing real uses it. Keep the README line that LaunchLab is blocked, as text. |
| D5 | Delete the template registry, `hook-template-sdk` and the CLI `template` command, or move them to their own repository? | Delete from this repository (move to a separate repo only if someone wants it). |
| D6 | Delete `xtask`'s `devnet` parts and `.github/workflows/devnet.yml`? | **Reversed: keep them.** I first called them maintainer-only, but `docs/forking.md` (public-cluster steps) and the Phase A hackathon runbook both tell a forker to deploy with `cargo xtask env deploy-devnet` under their own keys. Removing it would break the path those documents describe. |
| D7 | Merge `reference-hook-onchain` into the starter? | Yes. The starter's default `rule.rs` is the max-transfer rule; the driver, tests and `xtask` point at it. Keep the environment key `reference_hook` for now to limit churn, rename later. |
| D8 | Keep `benches/` in the default workspace? | Keep, but do not run its smoke test in `cargo test --workspace` (it is `#[ignore]` already; CI runs it explicitly). |
| D9 | Who may approve a hooked mint for pool creation, and how? | Admin only, through a new CLI command (Phase A). The approval instruction is signable only by the program's compile-time admin or one fixed owner key, so a fork or hackathon operator who deploys the forks with their own keys approves mints themselves. Do not change the fork to admit `TransferHook` without approval: it would be a one-line change that Raydium upstream would not accept. |

## Phases

Every phase ends with: `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets
-- -D warnings`, `cargo test --locked --workspace`, and the flows for what the phase touched. A phase
is not done on a failing check, and nothing is marked verified that was not run.

### Phase 0: baseline (before touching anything)

* Record the numbers to compare against: per-example compute units from
  `RTH_PROFILE=integration cargo test -p program-test-flows -- --ignored --nocapture`, the artifact
  sizes in `target/integration-sbf`, the SHA-256 of each `.so`, and the line count per group above.
* Confirm CI is green on `main`, on a warm cache. Tag the commit (`pre-restructure`) so every phase
  can be compared with, or reverted to, a known state.

Exit: baseline saved in `target/tmp/baseline.md` (not committed), tag pushed.

### Phase 1: prune (pure deletions; nothing is rewritten)

1. **The model island (D4).** Remove `integrations/cpmm`, `integrations/clmm`,
   `integrations/launchlab`, `crates/reference-hook-model`, `tests/models` from the workspace
   `members` and from disk.
2. **The template registry (D5).** Remove `programs/hook-template-registry`,
   `crates/hook-template-sdk`, and the CLI's `template` command
   (`crates/raydium-hook-cli/src/commands/template.rs`, its entry in `args.rs` and the dispatcher,
   the `hook-template-sdk` dependency). Remove the registry's `.so` from any build lists.
3. **`xtask` devnet tooling (D6): kept.** Forkers use it to deploy and record evidence under their own
   keys (see `docs/forking.md` and the Phase A runbook).
4. Regenerate `Cargo.lock` (`cargo metadata` / `cargo check`, then confirm `--locked` passes).
5. Fix every document that names what was removed: `README.md` (the `integrations/` row, the
   registry and `template` mentions), `docs/README.md`, `docs/permissionless-hooks.md`,
   `docs/architecture.md`, `docs/forking.md`, `CONTRIBUTING.md`.

Exit: the full check set passes; the localnet validator run (`cargo xtask localnet e2e`) still
passes on CI; `grep` for each removed name returns nothing outside history.

### Phase 2: the complete starter, and the reference hook folded into it

Add to `templates/transfer-hook-starter` (and `crates/hook-kit` per D3), each as an opt-in module
with its own unit tests and a short section in the starter README:

1. **Transfer context** (`hook-kit`): a struct carrying source, destination, mint, amount, both
   balances before and after, and the owner of each side; `is_buy(source, venues)`. Pure, no Solana
   types in the rule's signature.
2. **Clock and slot counters** (starter): read the clock, and a per-mint counter account that resets
   when the slot changes. Replaces the per-slot state written separately in `anti-bundle` and
   `fair-launch`.
3. **Holder records** (starter): create a record on first receipt (with the funding rule for its
   rent stated and tested), settle it on every transfer, never trap a transfer on a missing record.
   Replaces the copies in `loyalty-rewards` and `parent-spin-off`.
4. **Priority-fee reader** (starter): read the declared compute-unit price from the instructions
   sysvar. Documented as best-effort: it does not exist on v1 transactions (SIMD-0385), where
   ComputeBudget instructions are ignored.
5. **Writable-extra pattern** (starter): the correct way to declare a writable PDA extra, plus the
   matching `allowed_writable` entry for the client side.
6. **Thickness check**: a unit test in the starter that prints the hook's extra-account count and
   fails above a configurable ceiling (default: the ceiling the benchmarks found, about 10
   PDA-derived extras).
7. **Client description**: generate `setup.json` and the driver-side hook description from the
   rule's parameters, so `e2e --hook-dir` works with no hand-written JSON.
8. **Fold in the reference hook (D7).** The starter's default `rule.rs` becomes the max-transfer
   rule. Port every `reference-hook-onchain` test into the starter. Repoint what depended on
   `reference_hook_onchain`: `crates/raydium-hook-driver` (`hooks/reference.rs`, its `Cargo.toml`),
   `crates/raydium-hook-cli/src/commands/deploy.rs`, `tests/program-test/src/lib.rs`,
   `xtask/src/localnet.rs` (`HOOKS`). Check the error codes first: the flows pin `0x700b` for the
   over-limit refusal and must still see exactly that code. Then remove `programs/reference-hook-onchain`.

Tests: real Token-2022 processor for every module (the existing starter tests are the pattern).

Exit: starter tests green; every flow that used the reference hook passes unchanged against the
starter-built hook; `e2e --hook-dir templates/transfer-hook-starter` passes on a local validator
through CPMM and CLMM, as it does today.

### Phase 3: fair-launch absorbs anti-bundle

* `fair-launch` switches to the Phase 2 modules (context, slot counter, priority-fee reader) and
  deletes its private copies.
* Add the anti-bundle configuration: `max_buys_per_slot` set and every other cap off. A test proves
  it refuses and allows exactly what the old `anti-bundle` did (port its test cases).
* Remove `templates/anti-bundle`; drop it from `xtask/src/localnet.rs` `HOOKS`, from
  `crates/raydium-hook-driver/src/hooks/` (`anti_bundle.rs`, `mod.rs`), from the CLI's `SHIPPED`
  list, from `tests/program-test/tests/local_flows.rs`, and from `crates/hook-policy-model` if it
  names it.

Exit: all fair-launch flows green in-process and in the localnet validator run; the old anti-bundle
refusals reproduce under the fair-launch configuration.

### Phase 4: creator-commitment, with the gaps closed

* Move onto the shared modules; the rule stays `check_outgoing(schedule, now, balance_after)`.
* **Add the missing cases**, written as tests first, so their outcome is known before any doc claims
  it:
  * the dedicated account's owner changed by `SetAuthority`: does the floor still hold? (It should,
    because the floor belongs to the account; prove it.)
  * `Burn` of locked tokens: Token-2022 does not call a transfer hook for it. State what the hook
    can and cannot stop, and whether the `ImmutableOwner` extension or a burn authority closes the
    hole.
  * a delegate transfer and a permanent-delegate transfer: confirm the hook runs for both.
  * the dedicated account being empty or never funded (the "disclosed allocation" is only as real as
    this account): document how a reader verifies the allocation (balance of the account, schedule in
    the config PDA).
* Add an on-chain **disclosure record** if cheap (the schedule is already in the config PDA; a
  documented read path may be enough). Decide in this phase, do not assume.

Exit: the new tests pass and the README states, per case, what is enforced and what is not.

### Phase 5: holder-rewards (loyalty + spin-off in one)

* Rename `templates/loyalty-rewards` to `templates/holder-rewards`; the mode is a config field:
  `ongoing` (top-ups allowed) or `one-time` (fund once, then refused, the spin-off rule).
* Reward source: keep manual funding, and add the documented path **collect fees, then fund the
  stream** using the CPMM `collect_*_fee_v2` instructions (a flow that does it end to end on the
  second pool of the liquidity flow, so the claim "fees feed the rewards" is run, not described).
* Reward periods: decide in this phase whether epochs are needed or whether one continuous stream
  with top-ups is the honest model; write the answer in the README either way.
* Spin-off: state plainly that the child token must exist and be funded into the vault first; the
  template does not create it.
* Remove `templates/parent-spin-off`; update the same lists as Phase 3.

Exit: ongoing and one-time modes both pass in-process; the fees-to-rewards flow passes in-process;
the one-time mode refuses a second funding with its own error code.

### Phase 6: everything that names the old templates

Grep for each removed or renamed name and fix every hit:

* `xtask/src/localnet.rs` (`HOOKS`)
* `crates/raydium-hook-cli` (`SHIPPED`, `deploy.rs`, `e2e.rs`, `hook.rs`)
* `crates/raydium-hook-driver/src/hooks/*` and `src/env.rs`
* `tests/program-test` (`local_flows.rs`, `src/lib.rs`), `crates/hook-policy-model`
* `environments/*.json` template keys, `Cargo.toml` workspace members and `Cargo.lock`
* `README.md`, `docs/*`, `CONTRIBUTING.md`, `.github/workflows/ci.yml`

Exit: `grep -rn "anti-bundle\|anti_bundle\|parent-spin-off\|parent_spin_off\|loyalty-rewards\|loyalty_rewards"`
returns only history (old devnet entries, this plan, the changelog).

### Phase 7: documentation

* Rewrite the README's layout and template sections: starter plus three examples, and a table of
  "what you edit" versus "what is shared". The layout table lists only what remains.
* **Add `docs/commercial-and-limits.md`** (new). It covers, with sources or measurements for each
  claim:
  * what each example is for and who would pay for it;
  * who pays rent and compute, and what a hooked swap costs (point at `benches/`);
  * the limit table: about 10 PDA extras (heap), packet limit and lookup tables, the priority-fee
    check on v1 transactions, per-slot rules and what bundles across slots or wallets still do;
  * Raydium's per-mint admission as a business gate, and that it is Raydium's decision;
  * what a hook can never do (it does not run for burns or owner changes; it cannot make a mint
    "safe", only constrain transfers).
* Update `docs/transfer-surface-matrix.md` and `benches/README.md` where they name templates.

Exit: every claim in the new doc is either measured here, tested here, or marked "not verified".

### Phase 8: verify end to end, then devnet

1. Full local pass (fmt, clippy `-D warnings`, workspace tests, all flows with
   `RTH_PROFILE=integration`, the benchmark smoke test).
2. Push; CI must be green on both jobs, including `cargo xtask localnet e2e` on the validator, and
   green again on a second run with a warm cache.
3. Devnet: because the devnet tooling is removed (D6), build and deploy by hand with
   `cargo build-sbf` and `solana program deploy` (or `raydium-hook deploy`, which stays), deploy the
   three merged programs under new ids (D2), run `raydium-hook e2e --record` for each through CPMM
   and CLMM, verify each deployed binary's hash against the recorded one, and update
   `environments/devnet.json` and `docs/devnet.md` by hand. Budget: about 1 SOL per program (the
   starter's deployment cost about 0.94 SOL; check the balance first).
4. Run the starter on devnet again (`e2e --hook-dir templates/transfer-hook-starter`).

Exit: PASS rows for every example on devnet, hashes match, devnet doc updated.

### Phase 9: cleanup and the real numbers

* Mark the old devnet entries superseded; do not delete the evidence.
* Re-run the line count and replace the estimate in this file with the measured before and after.
* Re-run the benchmarks only if `hook-kit` changed its prelude.

### Phase A: mint approval command (independent; do any time after Phase 1)

**Why.** Creating a pool with a hooked mint needs a `SupportMintAssociated` record for that mint,
created by `create_support_mint_associated` on CPMM and on CLMM. The program accepts only two
signers: the compile-time `admin` and one fixed `create_support_mint_associated_owner` key. Today
only the end-to-end flows send that instruction (as the admin), and there is no way for a person to
do it. A fork or a hackathon operator needs one.

**What was investigated, and what it means**

* `bundle-launchlab-allow-configs.ts` and `fetch-launchlab-global-configs.ts` are about **LaunchLab**:
  reading its `GlobalConfig` accounts and creating `platform_allow_config` accounts for a LaunchLab
  platform. That is a different program and a different mechanism from the CPMM and CLMM mint
  approval, and LaunchLab is blocked here (its handler is not public). They do not do what this
  phase needs.
* They do show good practice to copy in spirit: pin the cluster by genesis hash, check an account's
  owner and discriminator before trusting it, fail closed on unknown layouts, simulate before
  sending, pack instructions into the 1,232-byte packet, and never print an RPC URL in an error.
* The Raydium SDK (`raydium-sdk-V2`, GPL-3.0, kept only as a local reference and never copied)
  derives the support-mint PDA and, when building a pool-creation transaction, adds the record to
  the remaining accounts. It has no builder for creating one. **No SDK is needed:** the instruction
  is a discriminator plus four accounts, and `crates/raydium-adapters` already builds it for both
  AMMs (`create_support_mint_instruction`), exercised by every flow.

**Build** (Rust, in `raydium-hook-cli`; no Node, no SDK):

1. `raydium-hook mint approve --env FILE --keypair ADMIN.json --mint M [--mint M2 ...]
   [--mints-file FILE] [--amm cpmm|clmm|all] [--dry-run]`
   * refuses unless the keypair is the admin the environment records, and says which key the
     program expects if not;
   * before sending, checks each mint with the existing readiness inspection: owned by Token-2022,
     has a TransferHook extension, the hook program is executable, the validation list is sound; a
     mint that fails is reported and skipped, not approved;
   * skips mints already approved (idempotent), simulates every transaction first, packs approvals
     into as few transactions as fit the packet, and `--dry-run` stops after the simulation;
   * prints a table of mint, AMM, result and signature.
2. `raydium-hook mint approval --env FILE --mint M`: read-only. For each AMM, whether the record
   exists (PDA owned by that program, discriminator and stored mint checked), so a team can see
   whether its mint was approved without asking an operator.
3. Docs: a "Approving a hooked mint" section in `docs/forking.md`, and a hackathon runbook there:
   the operator deploys the forks with the `integration` feature and their own keys (about 9 SOL of
   rent on devnet for both programs: the CPMM program account held 3.03 SOL at 597 KB, and CLMM is
   about twice the size), runs `mint approve` for each team's mint, and teams
   check with `mint approval`. Teams that do not want a shared cluster run everything locally, where
   the throwaway admin already approves mints.

**Tests**

* Unit: the instruction's discriminator and account order pinned for both AMMs; PDA derivation;
  batch packing stays under the packet limit; an already-approved mint is skipped.
* In-process flow: approving, then creating a pool with the hooked mint, succeeds; creating the pool
  **without** approval is refused by the program, so the gate itself is proven, not assumed.
* Local validator (CI): the new command run against the validator in the existing end-to-end job.
* Devnet: approve a fresh hooked mint with the command, then create a pool with it.

**Exit.** A person with only the admin key and this repository can approve a hooked mint and see it
approved, on localnet and on a cluster they deployed the forks to.

**Not in this phase.** A TypeScript version (it belongs with the TypeScript client plan and would
re-derive the same bytes); an HTTP approval service (a shell loop around the command is enough for
a hackathon); changing who may approve (D9).

## Risks

| Risk | Mitigation |
|---|---|
| A shared module changes behaviour and a merged example silently differs from the old one | Port every old test case into the merged crate; compare Phase 0 compute numbers; any increase above noise is explained or fixed. |
| Compute or heap grows as plumbing moves into shared modules (heap is already the limit at about 10 extras) | Thickness check in Phase 2; measure before and after; fail the phase on a regression. |
| Folding the reference hook into the starter changes an error code or a PDA the flows pin | Check the codes first (`0x700b` for the over-limit refusal); keep the old tests as the acceptance test. |
| Removing crates breaks something that only CI exercises (the localnet validator run) | Phase 1 is deletions only and is pushed alone so CI proves it before anything is rewritten. |
| Removing the devnet tooling makes the next devnet run manual | Accepted (D6); the commands are listed in Phase 8. If it hurts, restore it as a separate maintainer crate. |
| Warm CI caches break the fork checkout (this happened once) | Already fixed in `xtask upstream`; Phase 8 includes a second CI run on a warm cache. |
| New program ids lose the existing devnet evidence | Old entries kept as history (D1, D2). |
| The approval command approves a mint that cannot work, or is run with the wrong key | It checks readiness first and skips failures, refuses a key that is not the recorded admin, and has `--dry-run`. |

## Not in this plan

* CLMM liquidity and fee collection with a hooked mint.
* A TypeScript client SDK (separate plan; the Rust SDK and its fixtures are its reference).
* Changing Raydium's per-mint admission (a policy decision for the repository owner).

## Definition of done

* `templates/` has exactly the starter plus three examples; each example's own code is its rule,
  its tests and its parameters.
* The workspace has 15 members (plus the starter), and nothing in it is unused by the real stack.
* Every check in the "Every phase" paragraph passes on `main`, CI green on a warm cache.
* Devnet shows PASS for the three examples and the starter through both AMMs, with matching hashes.
* `raydium-hook mint approve` and `mint approval` exist, are tested (including the unapproved-mint
  refusal), and `docs/forking.md` has the approval steps and the hackathon runbook.
* The README and `docs/commercial-and-limits.md` state what is enforced, what is not, and what was
  not verified.

## Order of commits

1. `Remove the unused model island, the template registry and the maintainer-only tooling` (Phase 1)
2. `Starter: shared modules; fold in the reference hook` (Phase 2)
3. `Fair launch absorbs anti-bundle` (Phase 3)
4. `Creator commitment: close and document the gaps` (Phase 4)
5. `Holder rewards: loyalty and spin-off in one` (Phase 5)
6. `Remove the superseded templates` (Phase 6)
7. `Docs: layout, commercial and limits` (Phase 7)
8. `Devnet evidence for the merged examples` (Phase 8)
9. `Mint approval command and runbook` (Phase A; independent, can land before any of the above after Phase 1)

Each commit builds and passes the checks on its own.
