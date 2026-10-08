# Raydium Transfer Hooks: Fair Launch Reference UI Plan

**Status:** implemented; see "Outcome" at the end  
**Repository:** `trilltino/raydium-transfer-hook`  
**Reviewed main:** `c94c7853a062214074c60a078f042ca210b78fa8`  
**Reference UI:** `trilltino/raydium_debugger` + current Raydium swap UI  
**Reference template:** `templates/fair-launch`

---

## 1. Goal

Build one polished reference frontend that proves the complete developer path:

```text
Fair Launch template
        ↓
TypeScript hook client
        ↓
Raydium SDK V2 for pool data + quote math
        ↓
hook-aware Raydium swap instruction
        ↓
wallet
        ↓
localnet / integration devnet
        ↓
Raydium program
        ↓
Token-2022
        ↓
Fair Launch Transfer Hook
```

The app is an **experimental reference UI** for the hook-aware Raydium deployments in this repo.
It must never imply that the custom instructions already exist in official Raydium deployments.

---

## 2. Use Fair Launch first

Use `fair-launch` as the first UI because:

- it survives the template restructure;
- `anti-bundle` is being folded into it;
- it maps directly onto a Raydium buy/sell screen;
- success and failure are easy to demonstrate visibly;
- the same shell can later be reused for `creator-commitment` and `holder-rewards`.

The policy is simple to show:

```text
launch window
max buy
max wallet
max buys per slot
max declared priority fee
```

A normal buy succeeds. An invalid buy is rejected by the hook and the UI shows the exact reason.

---

## 3. Target the final repo shape

Do not build against crates that the restructure removes.

Expected final relevant shape:

```text
crates/
  transfer-hook-sdk/
  hook-policy-model/
  hook-kit/
  raydium-adapters/
  raydium-hook-driver/
  raydium-hook-cli/

templates/
  transfer-hook-starter/
  fair-launch/
  creator-commitment/
  holder-rewards/

programs/
  arbitrary-test-hook/
  bench-hook/

benches/
tests/
xtask/
```

Add:

```text
packages/
  transfer-hook-client/        TypeScript hook-aware Raydium client

apps/
  fair-launch-ui/              React/Vite reference frontend
```

Do not make a separate `anti-bundle` frontend.
Do not build the first UI around `loyalty-rewards`, because it becomes `holder-rewards`.

---

## 4. Responsibility split

### Raydium SDK V2

Use `@raydium-io/raydium-sdk-v2` for:

- Raydium initialization;
- token accounts;
- pool data;
- CPMM reserves/config;
- quote calculation;
- fee calculation;
- normal Raydium types and pool semantics.

Pin the reviewed version:

```json
"@raydium-io/raydium-sdk-v2": "0.2.73-alpha"
```

For CPMM quoting, follow the Raydium SDK V2 demo pattern using `getPoolInfoFromRpc` and `CurveCalculator.swapBaseInput`.

### Transfer Hook client

Do **not** call `raydium.cpmm.swap()` for the hooked transaction.

That builder emits the existing Raydium swap instruction. It does not know this repo's:

```text
swap_base_input_v2
swap_v3
```

or their hook-account counts.

The TypeScript client in this repo owns:

- reading the Token-2022 TransferHook extension;
- resolving each leg's `ExtraAccountMetaList`;
- validating hook accounts;
- keeping input/output hook slices separate;
- building `swap_base_input_v2`;
- building `swap_v3`;
- simulation;
- structured hook errors.

Use the SPL Token JavaScript helper:

```ts
addExtraAccountMetasForExecute(...)
```

Do not reimplement SPL TLV/PDA resolution.

### React UI

The UI owns:

```text
wallet
forms
quote display
policy display
simulation state
transaction state
error presentation
```

It must not contain raw instruction byte layouts or Transfer Hook seed logic.

---

## 5. TypeScript package

Create:

```text
packages/transfer-hook-client/
  package.json
  tsconfig.json
  src/
    index.ts
    environment.ts
    hook/
      read-hook.ts
      resolve-leg.ts
      privileges.ts
      errors.ts
    raydium/
      cpmm.ts
      clmm.ts
      quote.ts
    fair-launch/
      addresses.ts
      decode.ts
      errors.ts
      types.ts
    transaction/
      simulate.ts
      send.ts
  test/
    fixtures.test.ts
    resolve-leg.test.ts
    cpmm-framing.test.ts
    clmm-framing.test.ts
    fair-launch.test.ts
```

Public API:

```ts
loadEnvironment()
readTransferHook()
resolveTransferHookLeg()
buildCpmmSwapBaseInputV2()
buildClmmSwapV3()
simulateHookAwareTransaction()
decodeTransferHookFailure()

getFairLaunchConfigAddress()
getFairLaunchCounterAddress()
readFairLaunchConfig()
readFairLaunchCounter()
decodeFairLaunchError()
```

### Required invariant

For one hooked transfer:

```text
slice = resolved extras + hook program + validation PDA
count = slice.length
```

For no hook:

```text
slice = []
count = 0
```

For a swap:

```text
input slice
then output slice
```

Never merge, sort or logically deduplicate the slices.

---

## 6. Rust/TypeScript ABI fixtures

Do not maintain two ABIs by hand.

Add:

```text
tests/fixtures/typescript/
  cpmm-swap-base-input-v2.json
  clmm-swap-v3.json
  fair-launch-config.json
  fair-launch-errors.json
```

Each fixture contains:

```text
discriminator
instruction data hex
fixed account order
remaining account order
hook counts
PDA addresses
```

Rust and TypeScript tests must agree on the same bytes and account ordering.

CI fails on drift.

---

## 7. Environment handling

The first frontend supports only:

```text
localnet
integration devnet
```

Do not expose an official-Raydium mode yet.

Generate browser environment constants from:

```text
environments/localnet.json
environments/devnet.json
```

Output:

```text
apps/fair-launch-ui/src/generated/environments.ts
```

Browser type:

```ts
type HookEnvironment = {
  name: string
  cluster: "localnet" | "devnet"
  rpcUrl: string
  cpmmProgramId: string
  clmmProgramId: string
  fairLaunchProgramId: string
}
```

Always show:

```text
Experimental Raydium Transfer Hook environment
Not an official Raydium deployment
```

---

## 8. First trading surface: CPMM

Ship CPMM first.

Reasons:

- simplest Raydium swap path;
- `swap_base_input_v2` is already proven;
- CPMM has the broadest hook-aware surface in the repo;
- easiest reference for community developers.

Keep the React layer AMM-agnostic:

```ts
interface HookAwareSwapAdapter {
  loadPool(poolId: PublicKey): Promise<PoolView>
  quote(input: QuoteInput): Promise<SwapQuote>
  buildSwap(input: BuildSwapInput): Promise<TransactionInstruction[]>
}
```

Implement:

```text
CpmmHookAwareSwapAdapter    required
ClmmHookAwareSwapAdapter    next phase
```

---

## 9. Fair Launch state

Current Fair Launch config PDA:

```text
["config", mint]
```

Current counter PDA:

```text
["counter", mint]
```

Decode (the layout after the template restructure; **it changed from the original single `poolVault`**):

```ts
type FairLaunchConfig = {
  mint: PublicKey
  venues: PublicKey[]            // one to four pool vaults of the hooked token; a transfer out of one is a buy
  windowStart: bigint
  windowEnd: bigint
  maxBuy: bigint                 // 0 = no per-buy cap
  maxWallet: bigint              // 0 = no per-account cap
  maxBuysPerSlot: number         // 0 = no per-slot budget
  maxPriorityMicroLamports: bigint // 0 = no priority-fee check
}
```

A limit of `0` switches that check off (at least one must be on). With only `maxBuysPerSlot` set and a
window of `0..i64::MAX` the same program is the old anti-bundle guard, so the UI should show only the
limits that are on.

Config account, 214 bytes, little-endian:

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | `b"FLCONFIG"` |
| 8 | 1 | bump |
| 9 | 32 | mint |
| 41 | 1 | venue count (1 to 4) |
| 42 | 128 | four 32-byte venue slots (unused slots are zero) |
| 170 | 8 | window start (i64) |
| 178 | 8 | window end (i64) |
| 186 | 8 | max buy (u64) |
| 194 | 8 | max wallet (u64) |
| 202 | 4 | max buys per slot (u32) |
| 206 | 8 | max priority fee, micro-lamports (u64) |

The validation list carries two extra accounts (the config and the counter), and a third, the
instructions sysvar, **only when `maxPriorityMicroLamports` is set**. The TypeScript client must resolve
whatever the list declares, not assume three. New error: `InvalidVenues` (`0xB00B`) at setup.

Counter:

```ts
type FairLaunchCounter = {
  slot: bigint
  buys: number
}
```

The UI may warn before signing, but the on-chain hook remains authoritative.

---

## 10. React app structure

Create:

```text
apps/fair-launch-ui/
  package.json
  vite.config.ts
  tsconfig.json
  index.html
  src/
    main.tsx
    App.tsx
    styles.css
    config.ts
    components/
      Header.tsx
      EnvironmentBadge.tsx
      SwapCard.tsx
      TokenAmountInput.tsx
      SwapSummary.tsx
      FairLaunchPolicy.tsx
      PolicyMeter.tsx
      TransactionStatus.tsx
      DeveloperDetails.tsx
    hooks/
      useRaydium.ts
      usePool.ts
      useFairLaunch.ts
      useSwapQuote.ts
      useHookAwareSwap.ts
      useWalletBalances.ts
    generated/
      environments.ts
```

Use React + Vite.

Use Solana wallet adapter packages compatible with the Web3.js line used by Raydium SDK V2.

No backend is required for the public localnet/devnet reference app.
Never put private RPC keys, deployer keys, admin keys or mint-authority key files in frontend code.

---

## 11. Page layout

Desktop:

```text
┌─────────────────────────────────────────────────────────────┐
│ Raydium Transfer Hooks        Devnet        Connect Wallet  │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│         ┌─────────────────────┐ ┌─────────────────────────┐  │
│         │    RAYDIUM SWAP     │ │ FAIR LAUNCH POLICY      │  │
│         │                     │ │                         │  │
│         │ From                │ │ Active · 18m remaining  │  │
│         │ [ token ][ amount ] │ │ Max buy      10,000     │  │
│         │          ⇅          │ │ Max wallet   50,000     │  │
│         │ To                  │ │ Buys/slot    2 / 3      │  │
│         │ [ token ][ quote  ] │ │ Priority cap 1,000      │  │
│         │                     │ │                         │  │
│         │ Min received        │ │ Hook program            │  │
│         │ Price impact        │ │ 7xyk...PQSu             │  │
│         │ Estimated fees      │ └─────────────────────────┘  │
│         │ Hook status         │                              │
│         │ [ Connect / Swap ]  │                              │
│         └─────────────────────┘                              │
│                                                             │
│               Developer details ▾                           │
└─────────────────────────────────────────────────────────────┘
```

Mobile:

```text
Header
Swap card
Fair Launch policy
Transaction result
Developer details
```

No horizontal scrolling.

---

## 12. Raydium-style interaction model

Borrow the current Raydium swap interaction pattern:

```text
From
To
Max
50%
wallet balance
direction toggle
large primary action
minimum received
price impact
estimated fees
price freshness
unknown-token confirmation
```

Add hook-specific rows:

```text
Hook
Launch Policy
```

Example:

```text
Hook          Fair Launch · Active
Launch Policy 4 protections
```

---

## 13. Fair Launch UX

### Buy

When output is the hooked token and the transfer leaves the configured pool vault, show:

```text
BUY PROTECTIONS ACTIVE
```

Display:

```text
Buy amount       8,000 / 10,000
Wallet after     31,500 / 50,000
Buys this slot   2 / 3
Priority fee     500 / 1,000 µ-lamports
```

If the intended transaction obviously violates a rule, show it before signing.

Example:

```text
This buy exceeds the Fair Launch max-buy rule.
The on-chain hook will reject it.
```

### Sell

Show:

```text
Fair Launch buy restrictions do not apply to this sell.
```

### Window ended

Show:

```text
Fair Launch window ended
Transfers are no longer restricted by this policy.
```

---

## 14. Transaction version

Use **v0 transactions** in the reference UI.

The Fair Launch priority-fee rule reads `SetComputeUnitPrice`.
That rule is meaningful for legacy/v0 and not reliable for v1 transactions.

Therefore:

```text
reference UI = VersionedTransaction v0
```

If `maxPriorityMicroLamports == 0`, show:

```text
Priority fee rule disabled
```

---

## 15. Swap flow

The Swap button does this in order:

```text
1. Require wallet.
2. Reload pool state.
3. Reload Fair Launch config/counter.
4. Reload token balances.
5. Calculate Raydium quote.
6. Calculate minimum received.
7. Read TransferHook extension for each leg.
8. Resolve input hook slice.
9. Resolve output hook slice.
10. Build hook-aware Raydium instruction.
11. Compile v0 transaction.
12. Simulate.
13. If simulation succeeds, request wallet signature.
14. Submit.
15. Confirm.
16. Refresh pool, balances and policy state.
17. Show signature/result.
```

Do not request a wallet signature before simulation unless a developer override is explicitly enabled.

---

## 16. Hook failure UX

Decode Fair Launch failures:

```text
0xB003  Buy exceeds max buy
0xB004  Wallet balance would exceed max wallet
0xB005  Slot buy limit reached
0xB006  Declared priority fee exceeds launch limit
```

Render:

```text
Swap blocked by Fair Launch

Reason
Wallet balance would exceed the configured launch limit.

No transaction was submitted.
```

Keep the raw RPC/program error inside `Developer details`.

Unknown hook failure:

```text
Transfer Hook rejected this transaction.
Error code: 0x....
No known Fair Launch mapping exists for this code.
```

---

## 17. Developer details

Collapsed by default.

Show:

```text
Environment
Raydium program
Pool
Hook program
Hooked mint
Validation PDA
Input hook accounts
Output hook accounts
Instruction
Transaction version
Simulation compute
Signature
Explorer link
```

Instruction label:

```text
CPMM  swap_base_input_v2
CLMM  swap_v3
```

This makes the UI useful as both a product demo and developer documentation.

---

## 18. Styling

Do not copy Raydium or `raydium_debugger` source code.
Write new CSS using the same visual language.

Use these design tokens:

```css
:root {
  --bg: #070b14;
  --bg-deep: #050812;
  --surface: #1b2440;
  --surface-2: #11182b;
  --surface-3: #0d1324;
  --field: #090f20;

  --border: rgba(116, 139, 190, 0.22);
  --border-strong: rgba(128, 154, 216, 0.34);

  --text: #f4f7ff;
  --muted: #93a3ca;
  --dim: #66769e;

  --cyan: #2fd3e6;
  --blue: #4b73ff;
  --purple: #8b5cf6;
  --magenta: #c552ff;

  --green: #35d59b;
  --amber: #f7c35f;
  --red: #ff6b86;

  --radius: 10px;
  --brand-gradient: linear-gradient(135deg, #38d5e8 0%, #5178ff 52%, #9d4dff 100%);
}
```

Page background:

```css
background:
  radial-gradient(circle at 18% 0%, rgba(76, 103, 255, 0.16), transparent 28rem),
  radial-gradient(circle at 76% 10%, rgba(48, 211, 230, 0.13), transparent 26rem),
  linear-gradient(180deg, #050811 0%, #070b14 38%, #060912 100%);
```

Visual rules:

```text
compact trading layout
dark navy surfaces
cool-blue 1px borders
cyan/blue/purple accents
small radius
subtle glow only on primary actions
large numeric amount inputs
muted secondary text
minimal animation
```

Keep it closer to Raydium and `raydium_debugger` than a generic crypto landing page.

---

## 19. Header

Use:

```text
Raydium Transfer Hooks
Swap
Docs
GitHub

[ Integration Devnet ] [ Connect Wallet ]
```

Do not recreate every tab from the main Raydium app.
The reference frontend is not the official Raydium app.

---

## 20. Responsive requirements

Desktop:

```text
>= 1100px
swap + policy side by side
```

Tablet:

```text
720px - 1099px
swap first
policy below
```

Mobile:

```text
< 720px
single column
48px inputs
44px minimum buttons
16px input text
no inaccessible truncated addresses
```

Support:

```text
prefers-reduced-motion
keyboard focus
screen-reader labels
visible error states
```

---

## 21. Wallet and safety

Never expose:

```text
deployer keys
Raydium admin keys
mint authority key files
upgrade keys
private RPC URLs
```

Wallets sign user transactions only.

Pool approval and program deployment remain CLI/operator jobs.

Before sending:

```text
simulate
verify expected Raydium program
verify expected hook program
```

After confirmation:

```text
refresh balances
refresh policy counter
show explorer link
```

---

## 22. Pool selection

Do not build a global pool browser first.

Use:

```text
?pool=<PUBKEY>
```

Optionally include a small known-demo-pool list per environment.

Validate:

```text
pool owned by expected integration Raydium program
pool contains configured hooked mint
Fair Launch poolVault matches pool hooked-token vault
```

Fail closed on mismatch.

---

## 23. Unknown token confirmation

Community hook tokens will often be absent from Raydium's default token list.

Show:

```text
Unknown token

Mint
<full address>

Hook
Fair Launch

Program
<full address>

[ I understand, continue ]
```

The mint address is the identity, not the token symbol.

---

## 24. Tests

### TypeScript client

Required:

```text
Rust/TS ABI fixture equality
N + 2 resolution
no-hook count = 0
input/output slices stay separate
unexpected writable rejected
wrong hook program rejected
wrong validation owner rejected
CPMM bytes exact
CLMM bytes exact
Fair Launch config decode exact
Fair Launch error mapping exact
```

### React

Required:

```text
wallet disconnected
wallet connected
loading pool
active policy
expired policy
buy below limits
max-buy violation
max-wallet violation
slot-limit violation
priority-fee violation
sell path
simulation failure
successful confirmation
unknown-token confirmation
mobile layout
```

### Browser E2E

Use the existing local validator and a test-only wallet adapter.

Prove:

```text
1 passing CPMM Fair Launch buy
1 hook-refused buy
1 sell
```

For the refused buy:

```text
simulation returns hook error
UI shows human-readable reason
transaction is not submitted
balances stay unchanged
```

---

## 25. CI

After the Rust restructure is green, add frontend CI.

Run:

```sh
npm ci
npm run typecheck
npm run test
npm run build
```

Then local-validator browser E2E:

```sh
cargo xtask localnet build
cargo xtask localnet validator
npm run test:e2e
```

Do not make deterministic CI depend on public devnet.

---

## 26. Root JS workspace

Add:

```json
{
  "private": true,
  "workspaces": [
    "packages/*",
    "apps/*"
  ]
}
```

Suggested scripts:

```json
{
  "scripts": {
    "ui:dev": "npm --workspace apps/fair-launch-ui run dev",
    "ui:build": "npm --workspace apps/fair-launch-ui run build",
    "ui:test": "npm --workspace packages/transfer-hook-client test && npm --workspace apps/fair-launch-ui test",
    "ui:e2e": "npm --workspace apps/fair-launch-ui run test:e2e"
  }
}
```

Cargo continues to manage Rust.
NPM workspaces manage frontend/client code.

---

## 27. License boundary

The Rust repo is Apache-2.0.
Raydium SDK V2 is GPL-3.0.

Do not copy Raydium SDK source into Rust crates.
Consume the published npm package.

Before public distribution, explicitly document a GPL-compatible license for the frontend package and keep the Rust crate licensing separate and clear.

---

## 28. Documentation

Add:

```text
docs/frontend.md
```

Keep it focused on:

```text
what the reference UI proves
how to run localnet
how to run the frontend
how Raydium SDK is used
why hook-aware instruction building is separate
how to point at a pool
how to read Fair Launch policy
how to copy the pattern for another hook
official-Raydium limitation
```

Root README quick start:

```sh
npm install
cargo xtask localnet build
cargo xtask localnet validator
npm run ui:dev
```

Then:

```text
http://127.0.0.1:5173/?pool=<DEMO_POOL>
```

---

## 29. Reuse for the other final examples

### Creator Commitment

Reuse the whole trading shell.
Replace only the policy panel with:

```text
creator account
locked total
currently locked
unlocked amount
cliff
vesting progress
```

### Holder Rewards

Reuse the whole trading shell.
Add:

```text
registered / not registered
earned rewards
reward rate
claimable
Register
Claim
```

The Raydium + Transfer Hook transaction layer remains shared.

---

## 30. Implementation order

### UI-0: freeze contracts

Do after the template restructure has stable names/layouts.

Confirm:

```text
Fair Launch PDA seeds
Fair Launch config layout
Fair Launch error codes
CPMM hook-aware ABI
CLMM hook-aware ABI
environment JSON shape
```

### UI-1: TypeScript client

Build:

```text
environment loader
Fair Launch decoder
Transfer Hook leg resolver
CPMM builder
simulation/error decoder
Rust/TS fixtures
```

Exit:

```text
TypeScript produces the same CPMM instruction as Rust.
```

### UI-2: Raydium SDK integration

Build:

```text
Raydium.load
wallet token accounts
CPMM RPC pool loading
CurveCalculator quote
fee/price-impact view model
```

Exit:

```text
browser can display a correct quote from integration pool state.
```

### UI-3: trading UI

Build:

```text
wallet
From/To
Max/50%
quote
minimum received
price impact
fees
simulate
send
confirmation
```

Exit:

```text
normal hook-aware CPMM trade completes from browser.
```

### UI-4: Fair Launch product layer

Build:

```text
policy state
countdown
limit meters
preflight warnings
human hook errors
```

Exit:

```text
passing buy succeeds and over-limit buy is visibly blocked by the hook.
```

### UI-5: Raydium-style design

Apply the styling system above.

Exit:

```text
desktop + mobile look like part of the Raydium developer ecosystem while clearly labelled experimental.
```

### UI-6: local E2E + CI

Exit:

```text
green TS tests
green frontend build
green browser E2E
green Rust CI
```

### UI-7: CLMM

Implement `ClmmHookAwareSwapAdapter`.
Reuse the same React UI.

Exit:

```text
same Fair Launch UI can switch between CPMM and CLMM demo pools.
```

### UI-8: community release

Add:

```text
docs/frontend.md
README quick start
screenshots
known limitations
copy-this-example guide
```

---

## 31. Definition of done

The frontend is done when:

- it targets the final `fair-launch` template;
- `packages/transfer-hook-client` exists;
- Rust/TS ABI fixtures match;
- the app initializes Raydium SDK V2;
- the app loads a real integration CPMM pool;
- Raydium SDK data/math produces the quote;
- the hooked transaction uses `swap_base_input_v2`, not the stock builder;
- SPL Transfer Hook JS helpers resolve dynamic accounts;
- simulation runs before wallet signing;
- a valid Fair Launch buy succeeds;
- an invalid buy is rejected by the hook and shown as a human-readable policy failure;
- a sell succeeds without pretending buy rules apply;
- balances and policy state refresh after confirmation;
- localnet and integration devnet work;
- the UI clearly says the program IDs are experimental, not official Raydium;
- no admin/deployer/private RPC secrets ship to the browser;
- frontend CI and local-validator E2E are green;
- desktop and mobile styling is recognizably Raydium-inspired;
- Creator Commitment and Holder Rewards can reuse the shell without rewriting the transaction layer.

---

## 32. Do not build in v1

Do not add:

```text
mainnet trading
fake official-Raydium mode
generic no-code hook builder
browser program deployment
browser Raydium-admin approval
private-key upload
LaunchLab integration
global pool indexer
separate anti-bundle app
duplicated SPL hook resolver
forked Raydium SDK
backend unless protected RPC requires one
```

Keep the first product simple:

```text
connect wallet
open a Fair Launch pool
see the policy
get a Raydium quote
swap
watch the hook allow or refuse it
```

That is enough to demonstrate the complete developer story.

---

## Outcome

Implemented, with these deviations and findings. Everything listed as passing was run.

| Plan item | Result |
|---|---|
| UI-0 contracts | Fixed against the restructured `fair-launch` (214-byte config, venues, `0xB003` to `0xB00B` errors). |
| UI-1 client | `packages/transfer-hook-client`, 46 tests. **Deviation:** instead of a second copy of the CPMM/CLMM bytes under `tests/fixtures/typescript/`, the client is checked against the Rust crate's existing golden files (`crates/transfer-hook-sdk/tests/golden`), which Rust already regenerates and guards. Only `fair-launch.json` (config bytes, PDAs, error codes) is a new Rust-written fixture, in `tests/fixtures/typescript/`. |
| UI-2 Raydium SDK | Pinned `0.2.73-alpha`; used for pool/config reads and quote math only. **Finding:** the SDK's `computeSwapAmount` ignores the swap direction when deciding where the creator fee is taken; the program's rule (`is_creator_fee_on_input`) does not, so the app decides that and uses the SDK only for the curve. The SDK lists the CLMM bitmap extension before the tick arrays; the fork expects it after, so the CLMM adapter reorders. |
| UI-3, UI-4 | Trading UI and the Fair Launch policy layer, with pre-sign warnings and readable hook errors. The warnings are advisory; the button stays enabled so the simulation, which runs the real hook, decides. The priority-fee limit is shown but the UI never declares a fee, so that rule is only exercised through the error mapping. |
| UI-5 | Raydium-style tokens as specified; desktop and phone screenshots in `docs/images/`. |
| UI-6 | 51 app tests, a production build, and a 9-test browser suite (a passing buy, a hook-refused buy, a sell, for both a CPMM and a CLMM pool, plus a phone layout check). It passes against a local validator and against integration devnet. The `frontend` CI job is green; the browser test runs in the `runtime` CI job (see the commit history for its result). **Note:** `solana-test-validator` has no Windows build, so `cargo xtask localnet validator` falls back to a Docker container there (it needs `seccomp=unconfined` for io_uring). |
| UI-7 CLMM | Done: the adapter is chosen from the pool's owner program; the same page handles both. |
| UI-8 docs | `docs/frontend.md`, README quick start, screenshots. No "known demo pool list": pools come from `?pool=`. |

Not built, by the plan's own scope: mainnet, an official-Raydium mode, LaunchLab, a pool indexer, a backend.
Not covered: Creator Commitment and Holder Rewards panels (the shell is built to take them; only the Fair Launch
panel exists), exact-output swaps and liquidity in the UI, wrapped SOL.
