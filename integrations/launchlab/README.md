# LaunchLab policy simulator (MODEL ONLY, `model` feature)

`LaunchPolicySimulator` applies the platform policy model to a launch lifecycle (hook setup before trading, hook state recorded at graduation) and can produce the SDK `ResolveOptions` a trade would have to satisfy. It is behind the `model` feature and is not an API.

The reviewed public LaunchLab SDK exposes instruction layouts and discriminators, but the on-chain handler is closed source (confirmed by the owner). Buy/sell CPI count, authority seeds, and graduation transfer behavior remain unknown, so there is no LaunchLab framing and no live integration.
