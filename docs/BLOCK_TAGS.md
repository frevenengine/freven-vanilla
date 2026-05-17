# Vanilla block tags

Vanilla declares only conservative semantic tags for blocks that currently exist.

Initial common tags:

- `freven:stones`: `freven.vanilla:stone`
- `freven:soils`: `freven.vanilla:coarse_dirt`, `freven.vanilla:dirt`, `freven.vanilla:grass`
- `freven:glass`: `freven.vanilla:glass`
- `freven:terrain_solids`: `freven.vanilla:coarse_dirt`, `freven.vanilla:dirt`, `freven.vanilla:grass`, `freven.vanilla:stone`
- `freven:transparent_blocks`: `freven.vanilla:glass`

Mods can append to common tags from their own `content.manifest` by declaring the same tag key with `replace = false` or by omitting `replace`, which defaults to additive behavior.

Example TOML:

    [[block_tags]]
    key = "freven:stones"
    blocks = ["example.mod:marble"]

Do not declare tags for content that does not exist yet. For example, Vanilla should not publish logs, leaves, saplings, flammable, or harvest taxonomy until those blocks and semantics are real.
