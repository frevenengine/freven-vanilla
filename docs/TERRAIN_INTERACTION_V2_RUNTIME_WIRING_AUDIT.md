# Terrain Interaction v2 Runtime Wiring Audit

Issue: `freven-engine#386`

Status: blocked by layering. This Vanilla PR must not wire the runtime v2
server path until the v2 validator is available from an SDK-owned or otherwise
shared crate that Vanilla can legally depend on.

## Layering Conclusion

Vanilla currently depends on SDK crates such as `freven_block_api`,
`freven_world_api`, and `freven_block_guest`. It does not depend on
`freven-engine`, and this PR must not add such a dependency or modify engine,
boot, or SDK repositories.

The available v2 validator/harness is implemented in the sibling engine repo at:

```text
freven-engine/crates/freven_client_engine/src/terrain_interaction.rs
```

That module owns the authoritative v2 ray trace, face/contact validation,
reach, occlusion, first-solid/support, placement-empty, and block-id-policy
checks. Reusing it directly from Vanilla would invert the dependency direction:
Vanilla gameplay would depend on engine internals. Copying it into Vanilla
would duplicate long-term validator math and risk divergent authority semantics.

Therefore Vanilla runtime wiring is intentionally not implemented here. The
validator must move or be extracted first, preferably into an SDK/shared surface
that both engine and Vanilla may depend on without layering inversion.

## Current Vanilla State

The runtime path is still legacy v1:

- `crates/freven_vanilla_essentials/src/action_payloads.rs` encodes and decodes
  target-position/face payloads for break/place.
- `crates/freven_vanilla_essentials/src/client/block_interaction.rs` builds
  payloads from prediction-aware presented cursor hits, but still emits v1
  target-only bytes.
- `crates/freven_vanilla_essentials/src/actions/break.rs` and
  `crates/freven_vanilla_essentials/src/actions/place.rs` decode v1 payloads and
  perform Vanilla-local center/target visibility checks.
- `crates/freven_vanilla_essentials/src/actions/targeting.rs` contains
  Vanilla-local ray/visibility helpers that are not the v2 contract validator.

This means the client already preserves local visual prediction behavior, but
the server-authoritative validation is not the v2 contract and must not be
treated as an rc10 fallback.

## Required Extraction Before Runtime Wiring

Move or expose the v2 validator so Vanilla can call one shared implementation
for server authority. The shared surface needs to include, at minimum:

- `validate_terrain_interaction_v2`
- `TerrainInteractionValidationPolicyV2`
- `TerrainInteractionValidationV2`
- accepted-hit metadata
- world/rules adapter traits, including loaded/not-loaded/out-of-bounds cell
  classification
- deterministic `TerrainInteractionRejectReasonV2` results

The extracted implementation must remain authoritative over:

- finite normalized ray validation
- authoritative interaction origin and reach
- face/contact-aware first-solid tracing
- target/support solidity
- placement cell emptiness
- placement support/normal consistency
- allowed block id policy
- stream and input sequence bounds where available

## Vanilla Wiring After Extraction

After the validator is shared legally, Vanilla should make these changes in one
runtime PR:

1. Replace v1 rc10 payload emission with an explicit v2 payload codec in
   `action_payloads.rs`, using SDK vocabulary types from `freven_block_api`:
   `TerrainInteractionIntentV2`, `TerrainInteractionKindV2`,
   `TerrainInteractionRayV2`, `TerrainInteractionHitV2`,
   `TerrainPlaceIntentV2`, and `TerrainInteractionRejectReasonV2`.
2. Encode the action kind, stream identity, input sequence, prediction
   transaction id, dependency ids, ray origin, ray direction, max distance, hit
   block, hit face, optional hit point, and place-specific support/placement
   block id fields.
3. Keep v1 only as an explicit legacy/non-rc10 test path if still needed. Do not
   retain a hidden target-position-only fallback in the rc10 path.
4. Build client payloads from prediction-aware presented cursor state while
   preserving local visual prediction. The client must include enough intent
   data for server validation and must not promote prediction to authority.
5. Add or expose an identity source for same-cell break/place chains. If
   pre-submit code cannot know the final `action_seq`, the engine-owned pending
   record must bind the assigned action sequence before network send. If
   Vanilla still cannot express dependency identity, document and test that
   boundary explicitly.
6. Replace the server handlers' local target-only validation with calls to the
   shared validator. Map deterministic v2 reject categories onto the current
   `ActionOutcome::Rejected` surface and any available action-result reason
   surface without weakening authority.

## Deferred Test Matrix

The runtime PR after extraction should cover:

- v2 payload roundtrip
- client break payload includes ray and hit contract fields
- client place payload includes support and placement cell
- server accepts valid v2 break
- server accepts valid v2 place
- server rejects out-of-reach
- server rejects occluded target
- server rejects occupied placement
- server rejects v1/legacy payload on the rc10 path
- client prediction is never used as server authority

## Deferred Boot and Smoke Work

After Vanilla runtime wiring lands, `freven-boot` still needs its dependency bump
and manual smoke must be rerun against the integrated stack. This audit does not
change boot, engine, or SDK state.
