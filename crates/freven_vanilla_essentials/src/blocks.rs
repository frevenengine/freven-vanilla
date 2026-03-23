use freven_block_sdk_types::{BlockDescriptor, RenderLayer};

pub(crate) const STONE_KEY: &str = "freven.vanilla:stone";
pub(crate) const DIRT_KEY: &str = "freven.vanilla:dirt";
pub(crate) const GRASS_KEY: &str = "freven.vanilla:grass";

#[inline]
pub(crate) fn stone_def() -> BlockDescriptor {
    BlockDescriptor::new(true, true, RenderLayer::Opaque, 0x8080_80FF, 1)
}

#[inline]
pub(crate) fn dirt_def() -> BlockDescriptor {
    BlockDescriptor::new(true, true, RenderLayer::Opaque, 0x6B4F_2AFF, 2)
}

#[inline]
pub(crate) fn grass_def() -> BlockDescriptor {
    BlockDescriptor::new(true, true, RenderLayer::Opaque, 0x3FA3_4DFF, 3)
}
