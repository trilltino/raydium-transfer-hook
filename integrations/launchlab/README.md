# LaunchLab lifecycle model

`src/lib.rs` models policy-selected hook setup before trading and records the hook/list state when graduating the same mint.

The reviewed public LaunchLab SDK exposes instruction layouts and discriminators, but the on-chain Rust handler was not available. The SDK's Token-2022 mint builder does not expose Transfer Hook extension initialization or validation-list setup. Buy/sell CPI count, authority seeds, and graduation transfer behavior remain unknown. This module is a local state model, not a patch to LaunchLab.
