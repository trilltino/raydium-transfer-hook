# Loyalty and linked-token rewards template

Use bounded sender/receiver checkpointing during transfers. Do not iterate over all holders during reward distribution.

Separate reward claims from transfers. A claim instruction can settle accrued quote-token rewards; parent-token ownership can accrue points for a later child-token claim or distribution.

No accounting, vault, mint, or claim instruction is implemented yet.

