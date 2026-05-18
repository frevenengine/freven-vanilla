# Vanilla Blocktype Authoring Schema v1

This document defines the intended Vanilla-owned blocktype/worldproperty authoring
schema.

This is not an engine-global schema. It is the first concrete game-owned
authoring profile built on top of Freven's generic canonical content graph.

The profile id is:

    freven.vanilla:blocktypes_v1

## Boundary

The engine consumes canonical Freven content:

- textures;
- materials;
- models;
- block visual bindings;
- content families;
- block tags;
- future gameplay/content graph declarations.

Vanilla may offer a friendlier authoring layer:

- blocktypes;
- worldproperties;
- shapes;
- texture bindings;
- variant rules;
- tags;
- future drops/sounds/behaviors.

The compiler/expander turns Vanilla source files into canonical content graph
declarations. Engine/runtime code must not special-case Vanilla source files,
Vanilla block names, or Vanilla folder conventions.

## Current rc10 state

As of rc10, Vanilla already has a semantic modular canonical layout:

    core_experiences/freven.vanilla/content.manifest
    core_experiences/freven.vanilla/content/textures/terrain.toml
    core_experiences/freven.vanilla/content/textures/tint.toml
    core_experiences/freven.vanilla/content/models/common.toml
    core_experiences/freven.vanilla/content/blocktypes/coarse_dirt.toml
    core_experiences/freven.vanilla/content/blocktypes/dirt.toml
    core_experiences/freven.vanilla/content/blocktypes/grass.toml
    core_experiences/freven.vanilla/content/blocktypes/glass.toml
    core_experiences/freven.vanilla/content/families/rock.toml
    core_experiences/freven.vanilla/content/families/soil_grass.toml
    core_experiences/freven.vanilla/content/tags/terrain.toml

Those files are still canonical manifest source files. They are grouped
semantically, but they are not yet the high-level Vanilla blocktype schema.

This document defines the target schema that future Vanilla tooling should
compile into that same canonical graph.

## Target source layout

The target source layout is:

    content/
      blocktypes/
        rock.toml
        soil.toml
        grass.toml
        glass.toml
      worldproperties/
        rock.toml
        fertility.toml
        grass_coverage.toml
        color.toml
      shapes/
        block/
          cube.toml
          cube_faces.toml
          topsoil.toml
          glass_framed.toml
      textures/
        terrain.toml
        tint.toml
      tags/
        terrain.toml

Exact grouping may evolve, but the source categories are stable:

| Category | Purpose |
| --- | --- |
| `blocktypes/` | semantic Vanilla block definitions and variant rules |
| `worldproperties/` | reusable variant/property sets |
| `shapes/` | Vanilla-owned shape/model source |
| `textures/` | texture declarations and texture-set names |
| `tags/` | semantic block tags and tag expansion |
| generated canonical output | compile result, not source truth |

## Blocktype source files

A blocktype file describes a family of Vanilla blocks.

Conceptual shape:

    schema = 1
    profile = "freven.vanilla:blocktypes_v1"

    [blocktype]
    code = "soil"
    class = "freven.vanilla:soil"
    shape = "freven.vanilla:shapes/block/topsoil"
    draw = "topsoil"

    [[variantgroups]]
    code = "fertility"
    load_from = "worldproperties/fertility"

    [[variantgroups]]
    code = "coverage"
    load_from = "worldproperties/grass_coverage"

    [textures]
    base = "freven.vanilla:textures/soil_{fertility}"
    grass_top = "freven.vanilla:textures/grass_{coverage}_top"
    grass_side = "freven.vanilla:textures/grass_{coverage}_side"

    [visual]
    model = "freven.vanilla:models/block/topsoil_overlay"

    [tags]
    add = ["freven:soils", "freven:terrain_solids"]

This is a target schema example, not the current compiled canonical file shape.

## Worldproperties

A worldproperty file declares a reusable variant axis.

Conceptual fertility example:

    schema = 1
    profile = "freven.vanilla:blocktypes_v1"

    [worldproperty]
    code = "fertility"

    [[variants]]
    code = "poor"
    display = "Poor"
    fallback_tint_rgba = "5B4632FF"

    [[variants]]
    code = "medium"
    display = "Medium"
    fallback_tint_rgba = "6F4E2DFF"

    [[variants]]
    code = "rich"
    display = "Rich"
    fallback_tint_rgba = "46362AFF"

Worldproperties are Vanilla source facts. They may compile into canonical content
families, generated materials, generated visuals, generated tags, or future
gameplay metadata.

They are not engine runtime ids.

## Shapes

A shape file describes Vanilla-owned shape/model source.

Conceptual cube example:

    schema = 1
    profile = "freven.vanilla:blocktypes_v1"

    [shape]
    code = "cube"
    kind = "cube_all"

Conceptual TopSoil example:

    schema = 1
    profile = "freven.vanilla:blocktypes_v1"

    [shape]
    code = "topsoil"
    kind = "cuboid_parts"
    material_slots = ["base", "grass_side", "grass_top"]

    [[parts]]
    name = "base"
    from = [0.0, 0.0, 0.0]
    to = [1.0, 1.0, 1.0]

    [[parts]]
    name = "grass_overlay"
    from = [0.0, 0.0, 0.0]
    to = [1.0, 1.0, 1.0]
    overlay = true

The compiler maps shapes to canonical `[[models]]` entries.

Renderer handles, atlas coordinates, GPU state, and runtime ids are not valid
shape authoring fields.

## By-type rules

Vanilla source may use by-type matching to avoid repeating the same values for
every generated block.

Rules:

- matching is deterministic;
- the most specific matching rule wins only if the profile defines that order;
- wildcard fallback must be explicit;
- ambiguous matches are compile errors;
- generated keys must be stable;
- matching must not depend on filesystem order or hash-map order.

Conceptual example:

    [textures_by_type]
    "soil-*-bare".all = "freven.vanilla:textures/soil_{fertility}"
    "soil-*-sparse".base = "freven.vanilla:textures/soil_{fertility}"
    "soil-*-sparse".grass_top = "freven.vanilla:textures/grass_sparse_top"
    "soil-*-normal".grass_top = "freven.vanilla:textures/grass_normal_top"

The compiler expands these rules into canonical materials and block visual
bindings.

## Texture binding

Texture fields reference stable namespaced texture keys or profile-local aliases.

Valid target output:

- canonical texture declaration;
- canonical material declaration;
- canonical visual material slot binding.

Invalid authoring fields:

- atlas coordinates;
- texture-array layer ids;
- GPU handles;
- renderer material slot ids;
- renderer/runtime ids;
- generated cache paths.

Texture bytes still live as authored assets under the content root. Sha256
validation remains strict. The existing `freven_boot content-assets update-sha`
workflow remains the hash maintenance path.

## Tags

A blocktype may add generated blocks to tags.

Conceptual example:

    [tags]
    add = ["freven:soils", "freven:terrain_solids"]

or:

    [[tags]]
    tag = "freven:stones"
    value = "rock-{rock}"

The compiler expands this into canonical `[[block_tags]]` entries.

Tags are semantic content facts for mods, tools, gameplay, and validation. They
are not renderer categories and not runtime ids.

## Drops, sounds, and behaviors

The v1 schema may reserve fields for drops, sounds, and behaviors, but this issue
does not require every gameplay system to exist yet.

Allowed as schema direction:

    [sounds]
    walk = "walk/grass"
    break = "block/dirt"

    [[behaviors]]
    name = "freven.vanilla:spread_grass"

    [[drops]]
    type = "block"
    key = "freven.vanilla:soil_{fertility}_bare"

Rules:

- behavior names are stable semantic keys;
- behavior implementation belongs to Vanilla/gameplay code or future runtime
  capability systems;
- the engine must not infer behavior from Vanilla source file names;
- unsupported behavior fields must fail honestly or remain documented as reserved.

## Compile output

The Vanilla profile compiler outputs canonical Freven content graph declarations.

For current rc10 visuals, output includes:

- texture declarations;
- material declarations;
- model declarations;
- block visual bindings;
- content families or fully expanded generated entries;
- block tags.

Current examples:

| Vanilla source concept | Canonical output |
| --- | --- |
| rock blocktype + rock worldproperty | `freven.vanilla:families/rock`, generated rock materials, visuals, tags |
| soil blocktype + fertility/coverage worldproperties | `freven.vanilla:families/soil_grass`, generated soil/grass materials, visuals, tags |
| glass blocktype | transparent material and block visual binding |
| grass blocktype | per-face material visual binding and tint-capable overlay inputs |

The exact output can be either family-based or expanded entries, as long as the
resolved canonical graph is stable and diagnostics preserve provenance.

## Provenance and diagnostics

The compiler must preserve source provenance.

A good error should include:

- selected profile id;
- source file;
- source kind;
- field path;
- generated canonical declaration kind;
- generated key;
- first and second source when duplicates occur;
- fix guidance.

Example:

    error: duplicate generated material key
    profile: freven.vanilla:blocktypes_v1
    source: content/blocktypes/soil.toml
    field: textures.grass_top
    generated declaration: materials
    key: freven.vanilla:block/grass_normal_top
    first source: content/blocktypes/grass.toml
    second source: content/blocktypes/soil.toml
    fix: move the shared material to a common source file or rename the generated key

## Modding boundary

A Vanilla mod can target:

- `freven.vanilla:blocktypes_v1`;
- the low-level canonical graph;
- both, if it opts in explicitly.

Default Vanilla modding should prefer the Vanilla profile.

A mod targeting the Vanilla profile should not need to understand engine renderer
internals. A zero-Vanilla standalone game should not need to use Vanilla profile
files at all.

## Compatibility

Vanilla profile compatibility should be checked through:

- profile id;
- profile version;
- source schema version;
- generated key stability;
- known worldproperty ids;
- required Vanilla gameplay/behavior capabilities;
- canonical graph compatibility.

Breaking profile changes require an explicit profile version bump.

## Non-goals for v1

This document does not implement:

- the actual compiler;
- every behavior;
- every drop/sound rule;
- runtime scripting;
- engine hardcoding of Vanilla schema;
- a requirement that all games use Vanilla authoring files.

## Implementation follow-ups

Expected follow-ups:

- implement the Vanilla profile compiler behind `freven_boot content compile`;
- add fixtures that compare Vanilla profile source to canonical graph output;
- migrate current canonical `content/blocktypes/*.toml` files to true high-level
  blocktype source files;
- add DevKit docs/templates for Vanilla mod authors;
- add diagnostics for duplicate generated keys and unsupported reserved fields.
