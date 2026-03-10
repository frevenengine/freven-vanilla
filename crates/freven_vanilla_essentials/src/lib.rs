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

use freven_api::blocks::{BlockDef, RenderLayer};
use freven_api::voxel::{CHUNK_SECTION_DIM, CHUNK_SECTION_VOLUME, section_index};
use freven_api::{
    ActionKindId, ChannelConfig, ChannelDirection, ChannelId, ChannelOrdering, ChannelReliability,
    ClientOutboundMessage, ClientOutboundMessageScope, ComponentCodec, ComponentId, LogLevel,
    MessageCodec, MessageConfig, MessageId, ModContext, ModDescriptor, ModSide, Side,
    WorldGenError, WorldGenInit, WorldGenOutput, WorldGenProvider, WorldGenRequest,
    WorldGenSection,
};
use freven_std::action_defaults::action_keys;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

mod actions;
mod character_controller;
mod client;
mod storage_ids;

const FLAT_WORLDGEN_KEY: &str = "freven.vanilla:flat";

pub const MOD_DESCRIPTOR: ModDescriptor = ModDescriptor {
    id: "freven.vanilla.essentials",
    version: "0.1.0",
    side: ModSide::Both,
    register,
};

const AIR_KEY: &str = "freven.engine:air";
const STONE_KEY: &str = "freven.vanilla:stone";
const DIRT_KEY: &str = "freven.vanilla:dirt";
const GRASS_KEY: &str = "freven.vanilla:grass";

static FLAT_BLOCKS: OnceLock<FlatBlockIds> = OnceLock::new();
static VANILLA_ACTION_KINDS: OnceLock<VanillaActionKinds> = OnceLock::new();
static VANILLA_ECHO_IDS: OnceLock<VanillaEchoIds> = OnceLock::new();
static VANILLA_NAMEPLATE_COMPONENT_ID: OnceLock<ComponentId> = OnceLock::new();
const ACTION_KIND_BREAK_KEY: &str = action_keys::BREAK;
const ACTION_KIND_PLACE_KEY: &str = action_keys::PLACE;
pub const MODMSG_CHANNEL_ECHO_KEY: &str = "freven.vanilla:mod.echo";
pub const MODMSG_REQUEST_KEY: &str = "freven.vanilla:echo.request";
pub const MODMSG_RESPONSE_KEY: &str = "freven.vanilla:echo.response";
pub const PLAYER_NAMEPLATE_COMPONENT_KEY: &str =
    freven_api::engine_components::PLAYER_NAMEPLATE_TEXT;
const MODMSG_EXAMPLE_PAYLOAD: &[u8] = b"hello from vanilla client";
static CLIENT_ECHO_SENT: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VanillaEchoIds {
    channel_id: ChannelId,
    request_id: MessageId,
    response_id: MessageId,
}

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

pub(crate) fn player_nameplate_component_id() -> Option<ComponentId> {
    VANILLA_NAMEPLATE_COMPONENT_ID.get().copied()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FlatBlockIds {
    stone: u8,
    dirt: u8,
    grass: u8,
}

pub fn register(ctx: &mut ModContext<'_>) {
    let channel_id = ctx
        .register_channel(
            MODMSG_CHANNEL_ECHO_KEY,
            ChannelConfig {
                reliability: ChannelReliability::Reliable,
                ordering: ChannelOrdering::Ordered,
                direction: ChannelDirection::Bidirectional,
                budget: None,
            },
        )
        .expect("vanilla essentials must register freven.vanilla:mod.echo channel");
    let request_id = ctx
        .register_message_type(
            MODMSG_REQUEST_KEY,
            MessageConfig {
                codec: MessageCodec::RawBytes,
            },
        )
        .expect("vanilla essentials must register freven.vanilla:echo.request message");
    let response_id = ctx
        .register_message_type(
            MODMSG_RESPONSE_KEY,
            MessageConfig {
                codec: MessageCodec::RawBytes,
            },
        )
        .expect("vanilla essentials must register freven.vanilla:echo.response message");
    let echo_ids = VanillaEchoIds {
        channel_id,
        request_id,
        response_id,
    };
    if let Err(existing) = VANILLA_ECHO_IDS.set(echo_ids)
        && *VANILLA_ECHO_IDS
            .get()
            .expect("vanilla echo ids must be initialized")
            != existing
    {
        panic!("vanilla echo ids must remain deterministic across runtime builds");
    }

    let air = ctx
        .register_block(AIR_KEY, air_def())
        .expect("vanilla essentials must register freven.engine:air block");
    let stone = ctx
        .register_block(STONE_KEY, stone_def())
        .expect("vanilla essentials must register freven.vanilla:stone block");
    let dirt = ctx
        .register_block(DIRT_KEY, dirt_def())
        .expect("vanilla essentials must register freven.vanilla:dirt block");
    let grass = ctx
        .register_block(GRASS_KEY, grass_def())
        .expect("vanilla essentials must register freven.vanilla:grass block");

    if air.0 != storage_ids::AIR_U8 {
        panic!("vanilla requires AIR (block id 0)");
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

    let nameplate_component_id = ctx
        .register_component(PLAYER_NAMEPLATE_COMPONENT_KEY, ComponentCodec::RawBytes)
        .expect("vanilla essentials must register freven.engine:player_nameplate_text component");
    if let Err(existing) = VANILLA_NAMEPLATE_COMPONENT_ID.set(nameplate_component_id)
        && *VANILLA_NAMEPLATE_COMPONENT_ID
            .get()
            .expect("nameplate component id must be initialized")
            != existing
    {
        panic!("vanilla component ids must remain deterministic across runtime builds");
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
        ctx.on_start_client(modmsg_start_client);
        ctx.on_client_messages(modmsg_client_messages);
        ctx.on_start_client(log_start_client);
    }

    if ctx.side() == Side::Server {
        ctx.on_start_server(log_start_server);
        ctx.on_server_messages(modmsg_server_messages);
    }

    ctx.register_character_controller(
        character_controller::HUMANOID_KEY,
        character_controller::humanoid_factory,
    )
    .expect("vanilla essentials must register freven.vanilla:humanoid character controller");
}

fn log_start_client(_api: &mut freven_api::ClientApi<'_>) {
    freven_api::emit_log(LogLevel::Info, "vanilla lifecycle: start_client");
}

fn log_start_server(_api: &mut freven_api::ServerApi<'_>) {
    freven_api::emit_log(LogLevel::Info, "vanilla lifecycle: start_server");
}

fn modmsg_start_client(_api: &mut freven_api::ClientApi<'_>) {
    CLIENT_ECHO_SENT.store(false, Ordering::Relaxed);
}

fn modmsg_client_messages(api: &mut freven_api::ClientMessagesApi<'_>) {
    let Some(ids) = VANILLA_ECHO_IDS.get().copied() else {
        return;
    };

    if !CLIENT_ECHO_SENT.load(Ordering::Relaxed) {
        let send_res = api.sender.send_msg(ClientOutboundMessage {
            scope: ClientOutboundMessageScope::ActiveLevel,
            channel_id: ids.channel_id.0,
            message_id: ids.request_id.0,
            seq: None,
            payload: MODMSG_EXAMPLE_PAYLOAD.to_vec(),
        });

        if send_res.is_ok() {
            CLIENT_ECHO_SENT.store(true, Ordering::Relaxed);
        }
    }

    for msg in api.inbound {
        if msg.channel_id == ids.channel_id.0
            && msg.message_id == ids.response_id.0
            && msg.payload == MODMSG_EXAMPLE_PAYLOAD
        {
            api.log(
                LogLevel::Info,
                format!(
                    "vanilla mod echo response channel_id={} message_id={} payload_bytes={}",
                    msg.channel_id,
                    msg.message_id,
                    msg.payload.len()
                ),
            );
        }
    }
}

fn modmsg_server_messages(api: &mut freven_api::ServerMessagesApi<'_>) {
    let Some(ids) = VANILLA_ECHO_IDS.get().copied() else {
        return;
    };

    for msg in api.inbound {
        if msg.channel_id != ids.channel_id.0 || msg.message_id != ids.request_id.0 {
            continue;
        }
        let _ = api.sender.send_to(
            msg.player_id,
            freven_api::ServerOutboundMessage {
                scope: msg.scope,
                channel_id: msg.channel_id,
                message_id: ids.response_id.0,
                seq: msg.seq,
                payload: msg.payload.clone(),
            },
        );
    }
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
        let mut blocks = vec![storage_ids::AIR_U8; CHUNK_SECTION_VOLUME];
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
