# CPMM flow model

`src/lib.rs` plans the two concrete transfer legs for base-input swaps, deposits, and withdrawals, preserving a distinct resolved account range for each leg.

Upstream source review found that the current CPMM helper invokes `TransferChecked` with only fixed transfer accounts and does not forward handler remaining accounts. This crate does not patch or execute that upstream program. Keep live V1 layouts unchanged; a future program fork must explicitly wire each per-transfer slice before enabling hooked traffic.
