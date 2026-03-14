# Freven Vanilla (Reference Experience)

Freven Vanilla is the reference experience for Freven.

It demonstrates the current public architecture honestly:

- builtin / compile-time authoring through `freven_world_api`
- runtime-loaded world authoring through `freven_world_guest_sdk`
- the same semantic registration model used by builtin and runtime-loaded world guests
- experience-driven content and per-mod config
- a reference gameplay surface that can be loaded by Freven boot/runtime layers

Repository contents:
- `crates/freven_vanilla_essentials`
- `core_experiences/vanilla`

The recommended public runtime-loaded gameplay path is
`freven_world_guest_sdk` on Wasm. Neutral `freven_guest_sdk` remains available
for platform-shaped guests, but Vanilla exists to demonstrate the explicit
world-owned path.

Engine internals remain private.
