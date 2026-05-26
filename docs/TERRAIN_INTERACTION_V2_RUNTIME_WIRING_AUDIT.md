# Terrain Interaction v2 Runtime Wiring Audit

Issue: `freven-engine#386`

Status: runtime wiring implemented after the validator moved into the SDK-owned
`freven_block_api::terrain_interaction` surface.

## Layering Conclusion

Vanilla currently depends on SDK crates such as `freven_block_api`,
`freven_world_api`, and `freven_block_guest`. It does not depend on
`freven-engine`, and this PR must not add such a dependency or modify engine,
boot, or SDK repositories.

The previous blocker is gone: the shared v2 validator is now available through
`freven_block_api::terrain_interaction`, so Vanilla can validate terrain
interactions without depending on engine internals or copying validator math.

## Current Vanilla State

The rc10 runtime path is v2:

- `crates/freven_vanilla_essentials/src/action_payloads.rs` encodes and decodes
  explicit v2 terrain interaction payloads using SDK vocabulary types.
- `crates/freven_vanilla_essentials/src/client/block_interaction.rs` builds
  payloads from prediction-aware presented cursor hits while preserving local
  visual prediction.
- `crates/freven_vanilla_essentials/src/actions/break.rs` and
  `crates/freven_vanilla_essentials/src/actions/place.rs` decode v2 payloads and
  call `validate_terrain_interaction_v2` before applying authoritative block
  mutations.
- `crates/freven_vanilla_essentials/src/actions/targeting.rs` only retains
  Vanilla policy constants; the old target-only reach/visibility helpers are no
  longer used by the rc10 path.

Legacy v1 payload helpers remain only for explicit stale/legacy tests. The rc10
handlers reject v1 target-position-only payloads.

## Runtime Boundary

The current client action submit API assigns `action_seq` after Vanilla builds
the opaque payload. Vanilla therefore encodes `action_seq: None` and an empty
`depends_on` list at pre-submit time. This is covered by
`pre_submit_identity_defers_action_seq_and_same_cell_dependencies`.

Same-cell dependency identity still needs an engine/action-layer binding point if
future runtime behavior requires final `action_seq` in the payload before
network send. Vanilla does not make client prediction authoritative to paper over
that boundary.

## Server Authority

The server remains authoritative for:

- authoritative interaction origin and reach policy;
- face/contact-aware first-solid tracing through the SDK validator;
- target/support solidity and placement emptiness;
- allowed place block id through Vanilla rules;
- authoritative compare-and-set block mutation.

## Test Matrix

The runtime wiring covers:

- v2 payload roundtrip;
- client break payload includes ray and hit contract fields;
- client place payload includes support and placement cell;
- server accepts valid v2 break;
- server accepts valid v2 place;
- server rejects out-of-reach;
- server rejects occluded target;
- server rejects occupied placement;
- server rejects v1/legacy payload on the rc10 path;
- client prediction is never used as server authority;
- pre-submit action-sequence/dependency boundary is explicit.

## Deferred Boot and Smoke Work

`freven-boot` still needs its dependency bump and manual smoke must be rerun
against the integrated stack. This audit does not change boot, engine, or SDK
state.
