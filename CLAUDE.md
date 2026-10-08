# CLAUDE.md

Read [`AGENTS.md`](AGENTS.md) first. It is the single instruction file for this repository.

Key rules, repeated so they are never skipped:

* Business logic normally belongs in `rule.rs`. Do not casually touch the Execute plumbing.
* Every template needs tests (valid, rejected, boundary, malformed config, unauthorized update).
* Every template documents its limitations and how the rule can be bypassed.
* Every template documents its authority and trust assumptions (config authority, upgradeability,
  whether the mint's hook can be swapped).
* No mainnet, no real keys, no Raydium code in this repository.
