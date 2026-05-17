use freven_block_sdk_types::{BlockDescriptor, RenderLayer};

pub(crate) const STONE_KEY: &str = "freven.vanilla:stone";
pub(crate) const GRANITE_KEY: &str = "freven.vanilla:granite";
pub(crate) const LIMESTONE_KEY: &str = "freven.vanilla:limestone";
pub(crate) const DIRT_KEY: &str = "freven.vanilla:dirt";
pub(crate) const GRASS_KEY: &str = "freven.vanilla:grass";
pub(crate) const COARSE_DIRT_KEY: &str = "freven.vanilla:coarse_dirt";
pub(crate) const GLASS_KEY: &str = "freven.vanilla:glass";

pub(crate) const SOIL_GRASS_VARIANT_COUNT: usize = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VanillaBlockVariant {
    pub(crate) fertility: &'static str,
    pub(crate) coverage: &'static str,
    pub(crate) key: &'static str,
    pub(crate) fallback_material: &'static str,
    pub(crate) debug_tint_rgba: u32,
}

pub(crate) const SOIL_GRASS_VARIANTS: [VanillaBlockVariant; SOIL_GRASS_VARIANT_COUNT] = [
    VanillaBlockVariant {
        fertility: "poor",
        coverage: "bare",
        key: "freven.vanilla:soil_poor_bare",
        fallback_material: "freven.vanilla:block/soil_poor",
        debug_tint_rgba: 0x5B4632FF,
    },
    VanillaBlockVariant {
        fertility: "poor",
        coverage: "sparse",
        key: "freven.vanilla:soil_poor_sparse",
        fallback_material: "freven.vanilla:block/soil_poor",
        debug_tint_rgba: 0x5B4632FF,
    },
    VanillaBlockVariant {
        fertility: "poor",
        coverage: "normal",
        key: "freven.vanilla:soil_poor_normal",
        fallback_material: "freven.vanilla:block/soil_poor",
        debug_tint_rgba: 0x5B4632FF,
    },
    VanillaBlockVariant {
        fertility: "medium",
        coverage: "bare",
        key: "freven.vanilla:soil_medium_bare",
        fallback_material: "freven.vanilla:block/soil_medium",
        debug_tint_rgba: 0x6F4E2DFF,
    },
    VanillaBlockVariant {
        fertility: "medium",
        coverage: "sparse",
        key: "freven.vanilla:soil_medium_sparse",
        fallback_material: "freven.vanilla:block/soil_medium",
        debug_tint_rgba: 0x6F4E2DFF,
    },
    VanillaBlockVariant {
        fertility: "medium",
        coverage: "normal",
        key: "freven.vanilla:soil_medium_normal",
        fallback_material: "freven.vanilla:block/soil_medium",
        debug_tint_rgba: 0x6F4E2DFF,
    },
    VanillaBlockVariant {
        fertility: "rich",
        coverage: "bare",
        key: "freven.vanilla:soil_rich_bare",
        fallback_material: "freven.vanilla:block/soil_rich",
        debug_tint_rgba: 0x46362AFF,
    },
    VanillaBlockVariant {
        fertility: "rich",
        coverage: "sparse",
        key: "freven.vanilla:soil_rich_sparse",
        fallback_material: "freven.vanilla:block/soil_rich",
        debug_tint_rgba: 0x46362AFF,
    },
    VanillaBlockVariant {
        fertility: "rich",
        coverage: "normal",
        key: "freven.vanilla:soil_rich_normal",
        fallback_material: "freven.vanilla:block/soil_rich",
        debug_tint_rgba: 0x46362AFF,
    },
];

#[inline]
pub(crate) fn stone_def() -> BlockDescriptor {
    BlockDescriptor::solid_material_cube("freven.vanilla:block/stone", 0x8080_80FF)
}

#[inline]
pub(crate) fn granite_def() -> BlockDescriptor {
    BlockDescriptor::solid_material_cube("freven.vanilla:block/granite", 0x8C85_80FF)
}

#[inline]
pub(crate) fn limestone_def() -> BlockDescriptor {
    BlockDescriptor::solid_material_cube("freven.vanilla:block/limestone", 0xC8C2_A8FF)
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
pub(crate) fn soil_grass_variant_def(variant: VanillaBlockVariant) -> BlockDescriptor {
    BlockDescriptor::solid_material_cube(variant.fallback_material, variant.debug_tint_rgba)
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
        let baseline = [
            stone_def(),
            granite_def(),
            limestone_def(),
            dirt_def(),
            grass_def(),
            coarse_dirt_def(),
        ];

        for def in baseline
            .into_iter()
            .chain(SOIL_GRASS_VARIANTS.into_iter().map(soil_grass_variant_def))
        {
            assert!(def.is_solid());
            assert!(def.is_opaque());
            assert_eq!(def.render_layer(), RenderLayer::Opaque);
            assert_eq!(def.visual_kind(), BlockVisualKind::MaterialKey);
        }
    }

    #[test]
    fn soil_grass_variants_have_stable_base_material_fallbacks() {
        assert_eq!(SOIL_GRASS_VARIANTS.len(), 9);

        for variant in SOIL_GRASS_VARIANTS {
            assert!(variant.key.starts_with("freven.vanilla:soil_"));
            assert!(
                variant
                    .fallback_material
                    .starts_with("freven.vanilla:block/soil_")
            );
            assert!(variant.debug_tint_rgba & 0xFF == 0xFF);
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
