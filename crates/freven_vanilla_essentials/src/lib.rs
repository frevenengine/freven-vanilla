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

pub(crate) use crate::blocks::STONE_KEY;
use crate::blocks::{DIRT_KEY, GRASS_KEY, dirt_def, grass_def, stone_def};
use freven_block_sdk_types::BlockRuntimeId;
use freven_mod_api::{
    ChannelConfig, ChannelDirection, ChannelOrdering, ChannelReliability, ComponentCodec, LogLevel,
    MessageCodec, ModSide, Side, emit_log,
};
use freven_volumetric_sdk_types::CHUNK_SECTION_DIM;
use freven_world_api::{
    ActionKindId, ChannelId, ClientOutboundMessage, ClientOutboundMessageScope, MessageConfig,
    MessageId, ModContext, ModDescriptor, WorldGenError, WorldGenInit, WorldGenOutput,
    WorldGenProvider, WorldGenRequest, WorldTerrainWrite,
};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

pub mod action_defaults;
pub mod action_payloads;
mod actions;
mod blocks;
mod character_controller;
mod client;
pub mod humanoid_input;

const FLAT_WORLDGEN_KEY: &str = "freven.vanilla:flat";

pub const MOD_DESCRIPTOR: ModDescriptor = ModDescriptor {
    id: "freven.vanilla.essentials",
    version: "0.1.0",
    side: ModSide::Both,
    register,
};

static FLAT_BLOCKS: OnceLock<FlatBlockIds> = OnceLock::new();
static VANILLA_ACTION_KINDS: OnceLock<VanillaActionKinds> = OnceLock::new();
static VANILLA_ECHO_IDS: OnceLock<VanillaEchoIds> = OnceLock::new();
const ACTION_KIND_BREAK_KEY: &str = action_defaults::action_keys::BREAK;
const ACTION_KIND_PLACE_KEY: &str = action_defaults::action_keys::PLACE;
pub const MODMSG_CHANNEL_ECHO_KEY: &str = "freven.vanilla:mod.echo";
pub const MODMSG_REQUEST_KEY: &str = "freven.vanilla:echo.request";
pub const MODMSG_RESPONSE_KEY: &str = "freven.vanilla:echo.response";
pub const PLAYER_NAMEPLATE_COMPONENT_KEY: &str = "freven.vanilla:player_nameplate_text";
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FlatBlockIds {
    stone: BlockRuntimeId,
    dirt: BlockRuntimeId,
    grass: BlockRuntimeId,
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

    ctx.register_block(STONE_KEY, stone_def())
        .expect("vanilla essentials must register freven.vanilla:stone block");
    ctx.register_block(DIRT_KEY, dirt_def())
        .expect("vanilla essentials must register freven.vanilla:dirt block");
    ctx.register_block(GRASS_KEY, grass_def())
        .expect("vanilla essentials must register freven.vanilla:grass block");

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

    let _ = ctx
        .register_component(PLAYER_NAMEPLATE_COMPONENT_KEY, ComponentCodec::RawBytes)
        .expect("vanilla essentials must register freven.vanilla:player_nameplate_text component");

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

fn log_start_client(_api: &mut freven_world_api::ClientApi<'_>) {
    emit_log(LogLevel::Info, "vanilla lifecycle: start_client");
}

fn log_start_server(_api: &mut freven_world_api::ServerApi<'_>) {
    emit_log(LogLevel::Info, "vanilla lifecycle: start_server");
}

fn modmsg_start_client(_api: &mut freven_world_api::ClientApi<'_>) {
    CLIENT_ECHO_SENT.store(false, Ordering::Relaxed);
}

fn modmsg_client_messages(api: &mut freven_world_api::ClientMessagesApi<'_>) {
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

fn modmsg_server_messages(api: &mut freven_world_api::ServerMessagesApi<'_>) {
    let Some(ids) = VANILLA_ECHO_IDS.get().copied() else {
        return;
    };

    for msg in api.inbound {
        if msg.channel_id != ids.channel_id.0 || msg.message_id != ids.request_id.0 {
            continue;
        }
        let _ = api.sender.send_to(
            msg.player_id,
            freven_world_api::ServerOutboundMessage {
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
        ensure_flat_block_ids(resolve_flat_block_ids(&init));
        Self {
            seed: init.seed,
            world_id: init.world_id,
        }
    }

    fn emit_flat_column(&self, request: &WorldGenRequest, output: &mut WorldGenOutput) {
        let ids = FLAT_BLOCKS
            .get()
            .expect("vanilla essentials block ids must be initialized before worldgen");
        let min_x = request.cx() * CHUNK_SECTION_DIM as i32;
        let min_z = request.cz() * CHUNK_SECTION_DIM as i32;
        let max_x = min_x + CHUNK_SECTION_DIM as i32;
        let max_z = min_z + CHUNK_SECTION_DIM as i32;

        let mut push_layer = |y: i32, block_id: BlockRuntimeId| {
            output.writes.push(WorldTerrainWrite::FillBox {
                min: (min_x, y, min_z).into(),
                max: (max_x, y + 1, max_z).into(),
                block_id,
            });
        };

        push_layer(0, ids.stone);
        push_layer(1, ids.stone);
        push_layer(2, ids.stone);
        push_layer(3, ids.dirt);
        push_layer(4, ids.dirt);
        push_layer(5, ids.grass);
    }
}

impl WorldGenProvider for FlatWorldGen {
    fn generate(
        &mut self,
        request: &WorldGenRequest,
        output: &mut WorldGenOutput,
    ) -> Result<(), WorldGenError> {
        output.writes.clear();
        self.emit_flat_column(request, output);
        Ok(())
    }
}

fn resolve_flat_block_ids(init: &WorldGenInit) -> FlatBlockIds {
    FlatBlockIds {
        stone: init
            .block_id_by_key(STONE_KEY)
            .expect("vanilla essentials worldgen requires resolved stone block id"),
        dirt: init
            .block_id_by_key(DIRT_KEY)
            .expect("vanilla essentials worldgen requires resolved dirt block id"),
        grass: init
            .block_id_by_key(GRASS_KEY)
            .expect("vanilla essentials worldgen requires resolved grass block id"),
    }
}

fn ensure_flat_block_ids(resolved: FlatBlockIds) {
    if let Err(existing) = FLAT_BLOCKS.set(resolved)
        && *FLAT_BLOCKS.get().expect("flat blocks must be initialized") != existing
    {
        panic!("vanilla essentials block ids must remain deterministic across runtime builds");
    }
}
