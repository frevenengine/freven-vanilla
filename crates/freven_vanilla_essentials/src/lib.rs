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
    ActionKindId, ModContext, ModDescriptor, ModSide, Side, WorldGenError, WorldGenInit,
    WorldGenOutput, WorldGenProvider, WorldGenRequest, WorldGenSection,
};
use freven_core::blocks::{BlockDef, RenderLayer, storage::AIR};
use freven_core::voxel::{CHUNK_SECTION_DIM, CHUNK_SECTION_VOLUME, section_index};
use std::sync::OnceLock;

mod actions;
mod character_controller;
mod client;

const FLAT_WORLDGEN_KEY: &str = "freven.vanilla:flat";

pub const MOD_DESCRIPTOR: ModDescriptor = ModDescriptor {
    id: "freven.vanilla.essentials",
    version: "0.1.0",
    side: ModSide::Both,
    register,
};

const AIR_KEY: &str = "freven.core:air";
const STONE_KEY: &str = "freven.vanilla:stone";
const DIRT_KEY: &str = "freven.vanilla:dirt";
const GRASS_KEY: &str = "freven.vanilla:grass";

static FLAT_BLOCKS: OnceLock<FlatBlockIds> = OnceLock::new();
static VANILLA_ACTION_KINDS: OnceLock<VanillaActionKinds> = OnceLock::new();
pub const CLIENT_PLUGIN_BLOCK_INTERACTION: &str = "freven.vanilla:block_interaction";
const ACTION_KIND_BREAK_KEY: &str = "freven.vanilla:break";
const ACTION_KIND_PLACE_KEY: &str = "freven.vanilla:place";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VanillaActionKinds {
    break_kind: ActionKindId,
    place_kind: ActionKindId,
}

pub(crate) fn break_action_kind_id() -> ActionKindId {
    VANILLA_ACTION_KINDS
        .get()
        .expect("vanilla action kinds must be initialized")
        .break_kind
}

pub(crate) fn place_action_kind_id() -> ActionKindId {
    VANILLA_ACTION_KINDS
        .get()
        .expect("vanilla action kinds must be initialized")
        .place_kind
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FlatBlockIds {
    stone: u8,
    dirt: u8,
    grass: u8,
}

pub fn register(ctx: &mut ModContext<'_>) {
    ctx.on_start_common(log_start_common);

    let air = ctx
        .register_block(AIR_KEY, air_def())
        .expect("vanilla essentials must register freven.core:air block");
    let stone = ctx
        .register_block(STONE_KEY, stone_def())
        .expect("vanilla essentials must register freven.vanilla:stone block");
    let dirt = ctx
        .register_block(DIRT_KEY, dirt_def())
        .expect("vanilla essentials must register freven.vanilla:dirt block");
    let grass = ctx
        .register_block(GRASS_KEY, grass_def())
        .expect("vanilla essentials must register freven.vanilla:grass block");

    if air.0 != AIR {
        panic!("vanilla essentials requires freven.core:air to be id 0");
    }

    let resolved = FlatBlockIds {
        stone: stone.0,
        dirt: dirt.0,
        grass: grass.0,
    };
    if let Err(existing) = FLAT_BLOCKS.set(resolved)
        && *FLAT_BLOCKS.get().expect("flat blocks must be initialized") != existing
    {
        panic!("vanilla essentials block ids must remain deterministic across runtime builds");
    }

    let break_kind = ctx
        .register_action_kind(ACTION_KIND_BREAK_KEY)
        .expect("vanilla essentials must register freven.vanilla:break action kind");
    let place_kind = ctx
        .register_action_kind(ACTION_KIND_PLACE_KEY)
        .expect("vanilla essentials must register freven.vanilla:place action kind");
    let action_kinds = VanillaActionKinds {
        break_kind,
        place_kind,
    };
    if let Err(existing) = VANILLA_ACTION_KINDS.set(action_kinds)
        && *VANILLA_ACTION_KINDS
            .get()
            .expect("vanilla action kinds must be initialized")
            != existing
    {
        panic!("vanilla action kinds must remain deterministic across runtime builds");
    }

    if ctx.side() == Side::Server {
        ctx.register_worldgen(FLAT_WORLDGEN_KEY, flat_factory)
            .expect("vanilla essentials must register freven.vanilla:flat worldgen");

        ctx.register_action_handler(break_kind, actions::r#break::BreakActionHandler)
            .expect("vanilla essentials must register freven:break action handler");

        ctx.register_action_handler(place_kind, actions::place::PlaceActionHandler)
            .expect("vanilla essentials must register freven:place action handler");
    }

    if ctx.side() == Side::Client {
        ctx.register_client_control_provider(
            client::control::HUMANOID_CONTROL_KEY,
            client::control::humanoid_control_provider_factory,
        )
        .expect("vanilla essentials must register freven.vanilla:humanoid_controls");

        ctx.on_start_client(client::block_interaction::start_client);
        ctx.on_tick_client(client::block_interaction::tick_client);
        ctx.on_start_client(client::nameplates::start_client);
        ctx.on_tick_client(client::nameplates::tick_client);
        ctx.on_start_client(log_start_client);
        ctx.on_client_app(register_client_plugins);
    }

    if ctx.side() == Side::Server {
        ctx.on_start_server(log_start_server);
    }

    ctx.register_character_controller(
        character_controller::HUMANOID_KEY,
        character_controller::humanoid_factory,
    )
    .expect("vanilla essentials must register freven.vanilla:humanoid character controller");
}

fn register_client_plugins(installer: &mut dyn freven_api::ClientAppInstaller) {
    installer.install_plugin(CLIENT_PLUGIN_BLOCK_INTERACTION);
}

fn log_start_common(_api: &mut freven_api::CommonApi<'_>) {
    tracing::info!("vanilla lifecycle: start_common");
}

fn log_start_client(_api: &mut freven_api::ClientApi<'_>) {
    tracing::info!("vanilla lifecycle: start_client");
}

fn log_start_server(_api: &mut freven_api::ServerApi<'_>) {
    tracing::info!("vanilla lifecycle: start_server");
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
        let ids = FLAT_BLOCKS
            .get()
            .expect("vanilla essentials block ids must be initialized before worldgen");
        let mut blocks = vec![AIR; CHUNK_SECTION_VOLUME];
        fill_layer(&mut blocks, 0, ids.stone);
        fill_layer(&mut blocks, 1, ids.stone);
        fill_layer(&mut blocks, 2, ids.stone);
        fill_layer(&mut blocks, 3, ids.dirt);
        fill_layer(&mut blocks, 4, ids.dirt);
        fill_layer(&mut blocks, 5, ids.grass);
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

fn air_def() -> BlockDef {
    BlockDef {
        is_solid: false,
        is_opaque: false,
        render_layer: RenderLayer::Opaque,
        debug_tint_rgba: 0x0000_0000,
        material_id: 0,
    }
}

fn stone_def() -> BlockDef {
    BlockDef {
        is_solid: true,
        is_opaque: true,
        render_layer: RenderLayer::Opaque,
        debug_tint_rgba: 0x8080_80FF,
        material_id: 1,
    }
}

fn dirt_def() -> BlockDef {
    BlockDef {
        is_solid: true,
        is_opaque: true,
        render_layer: RenderLayer::Opaque,
        debug_tint_rgba: 0x6B4F_2AFF,
        material_id: 2,
    }
}

fn grass_def() -> BlockDef {
    BlockDef {
        is_solid: true,
        is_opaque: true,
        render_layer: RenderLayer::Opaque,
        debug_tint_rgba: 0x3FA3_4DFF,
        material_id: 3,
    }
}
