# Platform policy

The platform selects at most one hook program. A launch can select a supported preset or parameters within that engine; it cannot replace the platform-selected program.

The policy model in `hook-policy-model` covers:

- **Disabled:** no launch can request a hook preset.
- **Optional:** the platform may configure an engine; a launch may use it or launch without a preset.
- **Mandatory:** an engine must be configured and is selected for every launch.

The model also names three authority policies: platform-retained, immutable-at-launch, and governed/timelocked. These are design labels only; no on-chain authority transfer or immutability mechanism is implemented.

The local reference engine exercises these labels: platform-retained updates require the configured platform authority, immutable-at-launch rejects updates, and governed/timelocked updates require a model authorization token. Only the first two rules are simple state checks; the timelock token does not prove a real delay or governance execution.

Before storing this policy in a Raydium account, inspect the exact serialization, padding, IDL, and upgrade compatibility. Do not assume unused bytes can be repurposed safely.
