//! Vanilla essentials compile-time mod.
//!
//! Responsibilities:
//! - register baseline vanilla world generation providers
//! - keep provider keys stable for experience manifests
//! - avoid dependencies on engine internals
//!
//! Extension guidance:
//! - add more providers under stable namespaced keys
//! - keep output in SDK worldgen section format

use freven_api::{
    ModContext, ModDescriptor, ModSide, WorldGenError, WorldGenInit, WorldGenOutput,
    WorldGenProvider, WorldGenRequest, WorldGenSection,
};
use freven_core::blocks::storage::{AIR, DIRT, GRASS, STONE};
use freven_core::voxel::{CHUNK_SECTION_DIM, CHUNK_SECTION_VOLUME, section_index};

const FLAT_WORLDGEN_KEY: &str = "freven.vanilla:flat";

pub const MOD_DESCRIPTOR: ModDescriptor = ModDescriptor {
    id: "freven.vanilla.essentials",
    version: "0.1.0",
    side: ModSide::Server,
    register,
};

pub fn register(ctx: &mut ModContext<'_>) {
    ctx.register_worldgen(FLAT_WORLDGEN_KEY, flat_factory)
        .expect("vanilla essentials must register freven.vanilla:flat worldgen");
}

fn flat_factory(init: WorldGenInit) -> Box<dyn WorldGenProvider> {
    Box::new(FlatWorldGen::new(init))
}

struct FlatWorldGen {
    #[allow(dead_code)]
    seed: u64,
    #[allow(dead_code)]
    world_id: Option<String>,
}

impl FlatWorldGen {
    fn new(init: WorldGenInit) -> Self {
        Self {
            seed: init.seed,
            world_id: init.world_id,
        }
    }

    fn build_sy0() -> Vec<u8> {
        let mut blocks = vec![AIR; CHUNK_SECTION_VOLUME];
        fill_layer(&mut blocks, 0, STONE);
        fill_layer(&mut blocks, 1, STONE);
        fill_layer(&mut blocks, 2, STONE);
        fill_layer(&mut blocks, 3, DIRT);
        fill_layer(&mut blocks, 4, DIRT);
        fill_layer(&mut blocks, 5, GRASS);
        blocks
    }
}

impl WorldGenProvider for FlatWorldGen {
    fn generate(
        &mut self,
        request: &WorldGenRequest,
        output: &mut WorldGenOutput,
    ) -> Result<(), WorldGenError> {
        let _ = request;
        output.sections.clear();
        output.sections.push(WorldGenSection {
            sy: 0,
            blocks: Self::build_sy0(),
        });
        Ok(())
    }
}

fn fill_layer(blocks: &mut [u8], y: usize, block_id: u8) {
    for z in 0..CHUNK_SECTION_DIM {
        for x in 0..CHUNK_SECTION_DIM {
            let idx = section_index(x, y, z);
            blocks[idx] = block_id;
        }
    }
}
