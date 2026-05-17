use freven_block_sdk_types::{BlockDescriptor, RenderLayer};

pub(crate) const STONE_KEY: &str = "freven.vanilla:stone";
pub(crate) const DIRT_KEY: &str = "freven.vanilla:dirt";
pub(crate) const GRASS_KEY: &str = "freven.vanilla:grass";
pub(crate) const COARSE_DIRT_KEY: &str = "freven.vanilla:coarse_dirt";
pub(crate) const GLASS_KEY: &str = "freven.vanilla:glass";

#[inline]
pub(crate) fn stone_def() -> BlockDescriptor {
    BlockDescriptor::solid_material_cube("freven.vanilla:block/stone", 0x8080_80FF)
}

#[inline]
pub(crate) fn dirt_def() -> BlockDescriptor {
    BlockDescriptor::solid_material_cube("freven.vanilla:block/dirt", 0x6B4F_2AFF)
}

#[inline]
pub(crate) fn grass_def() -> BlockDescriptor {
    BlockDescriptor::solid_material_cube("freven.vanilla:block/grass", 0x3FA3_4DFF)
}

#[inline]
pub(crate) fn coarse_dirt_def() -> BlockDescriptor {
    BlockDescriptor::solid_material_cube("freven.vanilla:block/coarse_dirt", 0x7A5A_40FF)
}

#[inline]
pub(crate) fn glass_def() -> BlockDescriptor {
    BlockDescriptor::material_cube(
        true,
        false,
        RenderLayer::Transparent,
        "freven.vanilla:block/glass",
        0x80D8_FFCC,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use freven_block_sdk_types::BlockVisualKind;

    #[test]
    fn terrain_blocks_use_material_keys() {
        for def in [stone_def(), dirt_def(), grass_def(), coarse_dirt_def()] {
            assert!(def.is_solid());
            assert!(def.is_opaque());
            assert_eq!(def.render_layer(), RenderLayer::Opaque);
            assert_eq!(def.visual_kind(), BlockVisualKind::MaterialKey);
        }
    }

    #[test]
    fn glass_is_solid_but_visually_transparent() {
        let def = glass_def();

        assert!(def.is_solid());
        assert!(!def.is_opaque());
        assert_eq!(def.render_layer(), RenderLayer::Transparent);
        assert_eq!(def.debug_tint_rgba(), 0x80D8_FFCC);
        assert_eq!(def.visual_kind(), BlockVisualKind::MaterialKey);
    }
}
