# Vanilla Visual Content Pack v1

This document describes the first player-visible Vanilla visual content pack for
the rc10 visual pipeline.

The goal is not final art direction. The goal is to make Vanilla pleasant enough
to launch and inspect while remaining a copyable reference for modders and
standalone-game authors.

## Included authored blocks

The v1 pack includes these Vanilla block visuals:

| Block key | Material key | Texture key | Notes |
| --- | --- | --- | --- |
| `freven.vanilla:stone` | `freven.vanilla:block/stone` | `freven.vanilla:textures/stone` | Opaque terrain stone. |
| `freven.vanilla:dirt` | `freven.vanilla:block/dirt` | `freven.vanilla:textures/dirt` | Opaque soil base. |
| `freven.vanilla:grass` | `freven.vanilla:block/grass` | `freven.vanilla:textures/grass` | Opaque grass-covered soil baseline. |
| `freven.vanilla:coarse_dirt` | `freven.vanilla:block/coarse_dirt` | `freven.vanilla:textures/coarse_dirt` | First soil-style variant. |
| `freven.vanilla:glass` | `freven.vanilla:block/glass` | `freven.vanilla:textures/glass` | Solid collision, non-opaque visibility, transparent render layer. |

## Texture policy

All v1 textures are authored PNG files under:

~~~text
core_experiences/freven.vanilla/content/textures/
~~~

The current pack uses small `16x16` RGBA textures because this is the stable rc10
voxel-block baseline and is easy for modders to inspect, replace, hash, and copy.

Each texture is declared in:

~~~text
core_experiences/freven.vanilla/content.manifest
~~~

The manifest stores a stable texture key, package-local path, and sha256 for each
authored PNG. Generated atlases, texture-array slots, GPU handles, and renderer
material slots are not authored Vanilla content.

## Material policy

Materials are declared in `content.manifest`.

Opaque terrain materials omit `render_layer`, which preserves the default opaque
material behavior.

Glass declares:

~~~toml
render_layer = "transparent"
~~~

That render policy is authored content data. It is not an engine special case for
the word "glass", and it is not a raw renderer slot.

The glass block descriptor also declares non-opaque transparent block visibility
so collision/selection can remain solid while rendering uses the transparent
material path.

## Block tags

The v1 pack publishes only tags for real Vanilla blocks:

- `freven:stones`
- `freven:soils`
- `freven:glass`
- `freven:terrain_solids`
- `freven:transparent_blocks`

These are semantic content tags for mods and tools. They are not runtime block
ids and not renderer categories.

## Variants and future block visual files

This pack includes one concrete soil-style variant, `freven.vanilla:coarse_dirt`.

Full family expansion and canonical data-driven block visual files remain the
long-term authoring model. The SDK schema already defines those shapes, but this
repo should only treat them as source-of-truth once the runtime/tooling path is
wired end-to-end for Vanilla content.

Until then, the material-key block descriptor bridge is the honest rc10 path:

~~~text
Vanilla block key -> stable material key -> authored material -> authored texture
~~~

When canonical block visual files become runtime source-of-truth, Vanilla should
migrate without changing stable public block/material/texture keys.

## Override guidance

A texture pack can replace a Vanilla texture by overriding the same texture key in
a higher-precedence content layer.

For example, a grass texture override should replace:

~~~text
freven.vanilla:textures/grass
~~~

The replacement should keep the same stable key, point to the new authored PNG,
and update sha256 in that layer's `content.manifest`.

A material override can replace the material key instead, for example:

~~~text
freven.vanilla:block/glass
~~~

That is the right place to change material policy such as transparent vs cutout
once the selected product/runtime supports the desired policy.

## Validation

The Vanilla repo keeps regression tests for:

- declared texture and material keys;
- texture file existence and PNG baseline shape;
- transparent glass material policy;
- block descriptors using material keys rather than debug-only colors;
- README/docs links for the visual reference and content pack.

Runtime/devkit validation should additionally run content/assets checks before
release packaging.
