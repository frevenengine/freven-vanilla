# Vanilla Docs

`freven-vanilla` is the reference experience repository.

Its role is to show:
- how a builtin / compile-time experience uses `freven_world_api`
- how the matching runtime-loaded gameplay path uses `freven_world_guest_sdk`
- how block/content registration, world queries/mutations, terrain-write
  worldgen, and provider defaults fit together
- how experience content, defaults, and per-mod config fit together
- how builtin gameplay stays on the same semantic system as runtime-loaded mods
- how first-party-only policy stays above the generic world stack

The recommended public gameplay mod authoring path is `freven_world_guest_sdk`
on Wasm. Neutral guests that stay on platform-shaped declarations use
`freven_guest_sdk` instead.
