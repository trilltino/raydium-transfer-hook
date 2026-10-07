# reference-hook-onchain

Deployable Token-2022 Transfer Hook program used as the reference hook for the Raydium CPMM and
CLMM transport work in this repository.

## What it enforces

One rule, nothing else: the `max-transfer-v1` template rejects a hooked transfer whose
`amount` is greater than a per-mint `limit` (inclusive, so `amount == limit` passes).

Allow lists, deny lists, timelocks, launch policies and platform-level configuration exist only as
models in `crates/reference-hook-model` and `crates/hook-policy-model`. This program does not
implement them.

Raydium source is not part of this repository. This crate is tested against locally built SBF
artifacts only (see `docs/source-lock.md`).

## Accounts per hooked mint

| Account | Seeds | Owner | Size |
|---|---|---|---|
| Config | `["hook-config", mint]` | this program | `256 + params_len` (264 for `max-transfer-v1`) |
| Validation list | `["extra-account-metas", mint]` | this program | 51 bytes |

The validation list holds exactly one seeds-based `ExtraAccountMeta`:
`Literal "hook-config"` + `AccountKey { index: 1 }` (the mint). Its bytes are identical for every
mint, so it never needs migration, and a hooked transfer carries exactly three extra accounts
after the Token-2022 transfer accounts: `config`, `hook program`, `validation list`.

Both accounts are created by one atomic `InitializeHook`, so a mint can never be left with a
config and no list. Account creation tolerates a pre-funded PDA (anyone can send lamports to a
deterministic address): it tops up to rent exemption, then `allocate` and `assign`.

## Config layout (version 1)

Little-endian, packed, fixed offsets. Parsed strictly (`HookConfig::decode`): exact length,
discriminator, version (fail closed), zero reserved bytes, hash.

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | `b"HKCONFIG"` |
| 8 | 1 | version (`1`) |
| 9 | 1 | bump of the config PDA |
| 10 | 1 | bump of the validation-list PDA |
| 11 | 1 | authority mode |
| 12 | 4 | template_version |
| 16 | 32 | mint |
| 48 | 32 | platform_config (reserved, must be zero) |
| 80 | 32 | config_authority (mode 2 only, zero otherwise) |
| 112 | 32 | template_id (`"max-transfer-v1"` zero padded) |
| 144 | 32 | config_hash = `sha256(template_id \|\| template_version_le \|\| params)` |
| 176 | 8 | flags (no flag defined, must be zero) |
| 184 | 8 | config_seq |
| 192 | 2 | params_len (at most 256) |
| 194 | 62 | reserved (must be zero) |
| 256 | n | params (`max-transfer-v1`: `u64` limit, 8 bytes, limit > 0) |

Execute derives both PDAs with `create_program_address` and the stored bumps instead of
`find_program_address`.

## Authority modes

Identity authority is who may call `InitializeHook`. Rule authority is who may change the rule
afterwards. A required authority that is `None` returns `AuthorityUnavailable`.

| Mode | Name | Init signer | Update / SetConfigAuthority signer |
|---|---|---|---|
| 0 | ExtensionAuthority | live `TransferHook.authority` | live `TransferHook.authority` |
| 1 | MintAuthority | live mint authority | live mint authority |
| 2 | Explicit | live `TransferHook.authority` (consent) | stored `config_authority` |
| 3 | Immutable | live `TransferHook.authority` | nobody (`ConfigImmutable`) |
| 4 and above | reserved (PlatformControlled) | `UnsupportedMode` | `UnsupportedMode` |

Modes 0 and 1 read the live mint state at update time, so authority rotation on the mint is
followed. Mode 2 can be rotated with `SetConfigAuthority`; passing a zero key is the one-way
transition to mode 3. The BPF upgrade authority of the deployed program is a third, out-of-band
authority: it can replace the code, and therefore the rule for every mint, regardless of the
per-mint mode. Deploy with the authority you intend to trust, or revoke it.

## Instructions

All data is an 8-byte ASCII discriminator followed by little-endian fields. Builders live in
`src/lib.rs`: `initialize_hook_instruction`, `update_config_instruction`,
`set_config_authority_instruction`, `config_address`, `validation_list_address`.

| Instruction | Discriminator | Fields | Accounts |
|---|---|---|---|
| InitializeHook | `HKINIT01` | `mode u8, template_version u32, flags u64, template_id [u8;32], config_authority [u8;32], params_len u16, params` | `config (w), validation_list (w), mint, authority (s), payer (s, w), system` |
| UpdateConfig | `HKUPDT01` | `expected_seq u64, template_version u32, flags u64, params_len u16, params` | `config (w), mint, authority (s)` |
| SetConfigAuthority | `HKSETAU1` | `new [u8;32]` | `config (w), mint, authority (s)` |
| Execute | SPL interface | `amount u64` | exactly `source, mint, destination, owner, validation_list, config`, all read-only |

`InitializeHook` verifies, in order: system program, signers and writability, authority mode,
`mint.owner == Token-2022`, TransferHook extension present and pointing at this program, the
authority required by the mode, template and params, canonical PDA addresses, then that neither
PDA exists.

Every mutating instruction after init checks, in order: config owner, discriminator and version,
mint match, bump-derived address, mode, signer, and that the mint still points to this program.
`UpdateConfig` additionally requires `expected_seq == config_seq` (`StaleConfigSeq`) and bumps
the sequence with checked arithmetic.

`Execute` rejects direct invocation: both the source and destination token accounts must carry
the Token-2022 `transferring` flag. It requires exactly six accounts, validates the validation
list length and TLV by hand before `check_account_infos` (a corrupt list returns an error and
never panics), and logs nothing.

### SPL list instructions are not aliased

`InitializeExtraAccountMetaList` and `UpdateExtraAccountMetaList` are rejected with
`SplInterfaceUnsupported`. They carry no authority mode, template or params, so an alias would
have to invent a rule for the mint, which would weaken the checks. Use `InitializeHook`.

### Not implemented

`MigrateConfig` and a one-shot legacy migration do not exist. The pre-release `["policy", mint]`
layout is a hard cut. Config version 2 or above is rejected (`UnsupportedVersion`), so any future
layout needs a new program version or a `MigrateConfig` instruction.

## Error codes

`ProgramError::Custom(code)`, codes from `0x7001` (see `HookError`, which also offers
`from_code`).

| Code | Name |
|---|---|
| `0x7001` | NotDirectInvocation |
| `0x7002` | MintOwnerNotToken2022 |
| `0x7003` | MintHookProgramMismatch |
| `0x7004` | MintHookExtensionMissing |
| `0x7005` | InvalidConfigPda |
| `0x7006` | InvalidConfigOwner |
| `0x7007` | InvalidConfigData |
| `0x7008` | UnsupportedVersion |
| `0x7009` | InvalidValidationList |
| `0x700a` | AccountOrderMismatch |
| `0x700b` | TransferExceedsLimit |
| `0x700c` | AuthorityMismatch |
| `0x700d` | AuthorityUnavailable |
| `0x700e` | AlreadyInitialized |
| `0x700f` | UnsupportedMode |
| `0x7010` | ParamsTooLarge |
| `0x7011` | HashMismatch |
| `0x7012` | StaleConfigSeq |
| `0x7013` | WrongAccountCount |
| `0x7014` | ConfigImmutable |
| `0x7015` | InvalidParams |
| `0x7016` | UnknownTemplate |
| `0x7017` | SplInterfaceUnsupported |

## Testing

```powershell
# Unit tests plus native ProgramTest tests (the hook runs natively unless SBF_OUT_DIR is set).
cargo test --locked --target-dir target/hook-target -p reference-hook-onchain --lib --test hook_program --test token_2022_transfer

# SBF evidence: build the program, then run the same tests against the .so.
cargo build-sbf --manifest-path programs/reference-hook-onchain/Cargo.toml --sbf-out-dir target/raydium-runtime-sbf
$env:SBF_OUT_DIR = (Resolve-Path target/raydium-runtime-sbf).Path
cargo test --locked --target-dir target/hook-target -p reference-hook-onchain --test token_2022_transfer --test hook_program -- --nocapture
```

Only runs with `SBF_OUT_DIR` set count as SBF evidence. `hook_program.rs` pre-loads mints and
token accounts as Token-2022 account data, which is how it reaches guard paths such as a forged
`transferring` flag that only Token-2022 can set on-chain.

The hooked CPMM swap test (`tests/cpmm_swap_base_input_v2_runtime.rs`) is `#[ignore]` and needs
a locally built SBF CPMM artifact at `target/raydium-runtime-sbf/raydium_cp_swap.so`.
