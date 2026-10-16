# Local patch to librespot-connect 0.8.0

Source: the published crates.io `librespot-connect-0.8.0.crate`, SHA-256
`4b2a21e9e19b906c6676b1a515fc5529d5414f69049f5a167dfb2ea3206e5a27`.
License: MIT; the upstream license is included in this directory.

`src/spirc.rs` adds `Spirc::new_with_state` with an optional bounded Tokio watch
sender. It publishes the cluster already returned by device registration, then
the clusters already received by the dealer and returned by normal state writes.
`Spirc::new` remains compatible.
There are no extra network requests and no changes to playback or authentication.

The application and diagnostic probe use the same patched crate. When upgrading
librespot, reapply or replace this small integration patch and verify registration,
device discovery, transfers and remote commands with the probe and desktop app.
