# Vanilla Visual Reference

Freven Vanilla is the first-party reference content stack for Freven visuals.

This document defines the current rc10 Vanilla visual boundary. It is intentionally
about ownership, author-facing asset identity, and validation, not about final art
quality.

## Current rc10 status

Vanilla currently uses the SDK material-key block descriptor bridge for the first
terrain blocks:

- `freven.vanilla:stone`
- `freven.vanilla:dirt`
- `freven.vanilla:grass`

Each block points at a stable Vanilla material key through
`BlockDescriptor::solid_material_cube(...)`.

Those material keys are backed by authored content entries in:

~~~text
core_experiences/freven.vanilla/content.manifest
~~~

The texture source files live under the Vanilla content root:

~~~text
core_experiences/freven.vanilla/content/textures/
~~~

This is the transition-era bridge between the old debug-color renderer path and
the long-term data-driven block visual model.

## Ownership rules

Vanilla owns Vanilla visual style.

The engine must not know what "grass", "dirt", "stone", "glass", or any other
Vanilla block means. The engine may resolve generic material, texture, model,
render-layer, tint, lighting, and mesh data, but Vanilla block identity and
Vanilla art direction belong to this repository.

Allowed in Vanilla content/code:

- stable namespaced block keys, for example `freven.vanilla:grass`;
- stable namespaced material keys, for example `freven.vanilla:block/grass`;
- stable namespaced texture keys, for example `freven.vanilla:textures/grass`;
- fallback debug tints that keep content visible when real assets are missing or
  unsupported;
- content manifest declarations for authored textures, materials, and tags.

Not allowed as Vanilla authoring surface:

- renderer material slots;
- atlas coordinates;
- texture-array layer ids;
- GPU handles;
- Bevy/wgpu handles;
- generated cache paths;
- engine-side special cases for Vanilla block names.

## Relationship to future block visual content

The long-term authoring model is data-driven block visual content:

~~~text
block key -> visual key -> model/material/tint/render policy
~~~

The current runtime already supports the material-key descriptor bridge, so
Vanilla can be a valid rc10 reference before full block visual files are wired
through all runtime/tooling layers.

When canonical block visual content files become runtime-supported, Vanilla
should migrate the block-to-material binding out of Rust descriptors and into
content data without changing the stable public Vanilla block/material/texture
keys.

## Relationship to follow-up issues

This document closes the architectural reference boundary for Vanilla visuals.

It does not attempt to finish all player-visible visual work.

Follow-ups remain separate:

- Vanilla visual content pack v1: real stone/dirt/grass/glass/soil assets,
  variants, and richer material setup.
- Vanilla visual validation scene/preset.
- Reference visual mod examples.

That separation keeps the rc10 architecture honest: #29 is about moving Vanilla
visual ownership onto authored content/assets and documenting the boundary,
while later issues improve the actual art library and validation scenes.

## Validation expectations

A healthy Vanilla visual reference should satisfy these checks:

- `content.manifest` declares texture entries for stone, dirt, and grass.
- `content.manifest` declares material entries for stone, dirt, and grass.
- Vanilla block descriptors reference stable material keys, not raw renderer ids.
- Texture files referenced by the manifest exist under the Vanilla content root.
- Texture files are valid square power-of-two PNGs for the voxel block baseline.
- Fallback debug colors remain present and diagnosable.
- README links to this document so modders can find the reference boundary.
