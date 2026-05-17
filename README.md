# Freven Vanilla (Reference Experience)

Freven Vanilla is the reference experience for Freven.

It demonstrates the current public architecture honestly:

- builtin / compile-time authoring through `freven_world_api`
- runtime-loaded world authoring through `freven_world_guest_sdk`
- the same world query/mutation/content/worldgen/runtime-service model used by
  builtin and runtime-loaded world guests
- experience-driven content and per-mod config
- a first-party gameplay surface layered above the generic world stack

Repository contents:
- `crates/freven_vanilla_essentials`
- `core_experiences/freven.vanilla`

The recommended public runtime-loaded gameplay path is
`freven_world_guest_sdk` on Wasm. Neutral `freven_guest_sdk` remains available
for platform-shaped guests, but Vanilla exists to demonstrate the explicit
world-stack path and to own first-party gameplay policy such as flat worldgen,
humanoid controls, break/place ids, and nameplate presentation.

Engine internals remain private.

## Visual reference

Vanilla is also the first-party reference content stack for the rc10 visual asset
pipeline.

The current Vanilla visual baseline uses authored texture/material declarations
in `core_experiences/freven.vanilla/content.manifest`, with stable material keys
referenced from the Vanilla block descriptors through the SDK material-key bridge.

See [Vanilla Visual Reference](docs/VANILLA_VISUAL_REFERENCE.md) for the ownership
rules, current bridge status, validation expectations, and follow-up boundaries.

The first player-visible visual pack is documented in [Vanilla Visual Content Pack v1](docs/VANILLA_VISUAL_CONTENT_PACK_v1.md).
