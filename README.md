# Freven Vanilla (Reference Experience)

Freven Vanilla is the reference experience for Freven.

It demonstrates the current public architecture honestly:

- builtin / compile-time authoring through `freven_world_api`
- the same semantic registration model used by runtime-loaded guests
- experience-driven content and per-mod config
- a reference gameplay surface that can be loaded by Freven boot/runtime layers

Repository contents:
- `crates/freven_vanilla_essentials`
- `core_experiences/vanilla`

The recommended public runtime-loaded mod path remains `freven_guest_sdk` on
Wasm. Vanilla is useful as the builtin reference path, not as a competing
guest-transport story.

Engine internals remain private.
