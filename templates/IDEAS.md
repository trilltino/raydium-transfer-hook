# Template ideas

Not templates. A short, honest list of hook ideas and what stands between each and a reviewable
template. Add yours here with the questions answered before opening a template PR; do not add a
template just to raise the count.

The questions: real user problem; why a Transfer Hook (not a program, not an off-chain check); bypasses;
state; extra accounts; contention; compute; authority/trust; is it better solved elsewhere.

| Idea | Real problem | Why a hook / better elsewhere | Main bypass or risk | State, contention | Verdict |
|---|---|---|---|---|---|
| Trading cooldown | stop wash/spam trading per account | hook sees every transfer; a program cannot | many accounts; sells from fresh accounts | per-account last-transfer record, **writable per holder** (low contention) | **Good candidate.** Small, clearly different from the three shipped |
| Allowlisted launch phase | only approved wallets receive in phase 1 | hook can reject at the token level | approved wallet forwards tokens; allowlist is a trust root | allowlist as PDAs per wallet (read-only), or a merkle root | **Good candidate.** Needs a clear admin model |
| Token-gated transfers | recipient must hold another token | hook can read the recipient's balance of that token | borrow the token for the transfer (flash) | one read-only extra account per check | Candidate; flash-borrow caveat must be documented |
| NFT / member-based rules | members trade, others cannot | same as above | transfer the NFT | read-only membership account | Candidate after token-gated |
| Progressive transfer unlock | max sendable grows with holding time | extends Creator Commitment to every holder | many accounts | per-holder record, writable | Overlaps Creator Commitment and Holder Rewards; wait |
| Treasury commitment | treasury cannot dump | a vesting floor on a treasury account | same bypasses as Creator Commitment | read-only config | Probably a parameterisation of Creator Commitment, not a new template |
| Parent-token / spin-off eligibility | child token for parent holders | already covered by Holder Rewards one-time mode | | | **Covered** |
| Loyalty accounting | reward long-term holders | already covered by Holder Rewards | | | **Covered** |
| Venue-specific policy | different rules per DEX/pool | hook can see source/destination | a new venue address; venue lists are per mint | read-only list | Fair Launch's venues already do part of this |
| Dynamic policy from transfer context | rules vary by amount, time, size | the starter's `TransferContext` is the extension point | complexity and compute | varies | Not a template; a pattern for `rule.rs` |
| Recipient qualification | KYC-style gating | **likely better solved elsewhere** (a permissioned token / compliance extension, off-chain attestations) | attestation freshness; who is the issuer | attestation account | Needs a trust model before any code |
| Controlled OTC / vesting flows | escrowed bilateral transfers | **likely better solved elsewhere** (an escrow program; a hook cannot move tokens) | | | Not a hook problem |
