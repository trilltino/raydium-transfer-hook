# Security

## Status

This repository is **not audited**. It is an independent, community project maintained in a
personal capacity, not affiliated with or endorsed by Raydium. The hooks here are reference
implementations and teaching material, provided "as is" under the [MIT License](LICENSE), without
warranty. Do not deploy any of them to mainnet without your own independent review.

## Reporting a vulnerability

Please report security issues **privately**, not in a public issue or pull request:

* Use GitHub's private vulnerability reporting:
  [Report a vulnerability](https://github.com/trilltino/raydium-transfer-hook/security/advisories/new)
  (the "Security" tab, then "Report a vulnerability").

Include what is affected (starter, `hook-kit`, which template), how to reproduce it (a failing test
is ideal), and the impact you expect (a transfer that should be refused passes, funds can be moved
or locked, a panic, ...).

## What to expect

* This is maintained on a best-effort basis. There is no bug bounty and no guaranteed response time.
* Confirmed issues are fixed in the repository with a regression test, and credited if you wish.
* Because nothing here is deployed by the maintainers as a production service, there is no
  coordinated disclosure with third parties. If you have deployed a hook derived from this code,
  you are responsible for upgrading or retiring it.

## In scope

* Ways around the shared checks (`Execute` from outside a transfer, wrong validation list, wrong
  authority at setup, PDA initialisation).
* A template behaving differently from what its README's RULES, LIMITATIONS or TRUST say.
* `scripts/deploy.sh` doing something unsafe (for example deploying to mainnet without
  `--allow-mainnet`).

Limitations a template already documents (per-account rather than per-person rules, contention,
burns being invisible to a hook, ...) are known design trade-offs, not vulnerabilities, unless the
documentation is wrong.
