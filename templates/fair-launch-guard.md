# Fair Launch Guard template

Potential modules: per-slot anti-bundle limits, max-wallet limits, trade-size caps, and launch-window rules.

Before implementation, define how transfers differ from trades, how exemptions work, and whether transaction introspection is used. Avoid a single global writable hotspot where possible. Validate incompatible module combinations during configuration, not on every transfer.

No guard logic is implemented yet.

