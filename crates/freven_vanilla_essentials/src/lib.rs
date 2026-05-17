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
use crate::blocks::{
    COARSE_DIRT_KEY, DIRT_KEY, GLASS_KEY, GRASS_KEY, coarse_dirt_def, dirt_def, glass_def,
    grass_def, stone_def,
};
use freven_avatar_api::{
    AvatarControlRegistrationExt, AvatarControllerRegistrationExt, AvatarLifecycleRegistrationExt,
    ClientApi,
};
use freven_block_sdk_types::BlockRuntimeId;
use freven_mod_api::{
    ChannelConfig, ChannelDirection, ChannelOrdering, ChannelReliability, ComponentCodec, LogLevel,
    MessageCodec, ModSide, Side, emit_log,
};
use freven_volumetric_api::{
    InitialWorldSpawnHint, WorldGenError, WorldGenInit, WorldGenOutput, WorldGenProvider,
    WorldGenRequest, WorldTerrainWrite,
};
use freven_volumetric_sdk_types::CHUNK_SECTION_DIM;
use freven_world_api::{
    ActionKindId, ChannelId, ClientOutboundMessage, ClientOutboundMessageScope, MessageConfig,
    MessageId, ModContext, ModDescriptor,
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
const VISUAL_VALIDATION_WORLDGEN_KEY: &str = "freven.vanilla:visual_validation";

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
    coarse_dirt: BlockRuntimeId,
    glass: BlockRuntimeId,
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
    ctx.register_block(COARSE_DIRT_KEY, coarse_dirt_def())
        .expect("vanilla essentials must register freven.vanilla:coarse_dirt block");
    ctx.register_block(GLASS_KEY, glass_def())
        .expect("vanilla essentials must register freven.vanilla:glass block");

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
        ctx.register_worldgen(VISUAL_VALIDATION_WORLDGEN_KEY, visual_validation_factory)
            .expect("vanilla essentials must register freven.vanilla:visual_validation worldgen");

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

fn log_start_client(_api: &mut ClientApi<'_>) {
    emit_log(LogLevel::Info, "vanilla lifecycle: start_client");
}

fn log_start_server(_api: &mut freven_world_api::ServerApi<'_>) {
    emit_log(LogLevel::Info, "vanilla lifecycle: start_server");
}

fn modmsg_start_client(_api: &mut ClientApi<'_>) {
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

fn visual_validation_factory(init: WorldGenInit) -> Box<dyn WorldGenProvider> {
    Box::new(VisualValidationWorldGen::new(init))
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

struct VisualValidationWorldGen {
    flat: FlatWorldGen,
}

impl VisualValidationWorldGen {
    fn new(init: WorldGenInit) -> Self {
        Self {
            flat: FlatWorldGen::new(init),
        }
    }

    fn emit_validation_scene(&self, request: &WorldGenRequest, output: &mut WorldGenOutput) {
        if request.cx() != 0 || request.cz() != 0 {
            return;
        }

        let ids = FLAT_BLOCKS
            .get()
            .expect("vanilla essentials block ids must be initialized before worldgen");

        output.bootstrap.initial_world_spawn_hint = Some(InitialWorldSpawnHint {
            feet_position: [16.5, 7.0, 24.5],
        });

        // Five material swatches on top of the flat terrain.
        let swatches = [
            (4, ids.stone),
            (8, ids.dirt),
            (12, ids.grass),
            (16, ids.coarse_dirt),
            (20, ids.glass),
        ];

        for (x, block_id) in swatches {
            output.writes.push(WorldTerrainWrite::FillBox {
                min: (x, 6, 6).into(),
                max: (x + 3, 7, 9).into(),
                block_id,
            });
        }

        // Transparent glass wall in front of an opaque stone marker.
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (6, 6, 14).into(),
            max: (14, 11, 15).into(),
            block_id: ids.glass,
        });
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (9, 6, 16).into(),
            max: (12, 10, 17).into(),
            block_id: ids.stone,
        });

        // Greedy-mesh UV probes. These large quads catch atlas bleeding,
        // stretching, and face-axis mistakes that small cubes can hide.
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (2, 6, 22).into(),
            max: (14, 7, 30).into(),
            block_id: ids.grass,
        });
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (2, 7, 30).into(),
            max: (14, 13, 31).into(),
            block_id: ids.stone,
        });
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (14, 7, 22).into(),
            max: (15, 13, 30).into(),
            block_id: ids.dirt,
        });
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (18, 7, 22).into(),
            max: (19, 13, 30).into(),
            block_id: ids.coarse_dirt,
        });

        // Small occlusion/shadow alcove: visible lighting contrast without
        // making the default flat world heavy or special-casing engine lighting.
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (20, 6, 12).into(),
            max: (28, 7, 20).into(),
            block_id: ids.stone,
        });
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (20, 10, 12).into(),
            max: (28, 11, 20).into(),
            block_id: ids.stone,
        });
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (20, 7, 12).into(),
            max: (21, 10, 20).into(),
            block_id: ids.stone,
        });
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (27, 7, 12).into(),
            max: (28, 10, 20).into(),
            block_id: ids.stone,
        });
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (20, 7, 12).into(),
            max: (28, 10, 13).into(),
            block_id: ids.stone,
        });

        // Face-lighting reference steps expose top, side, and underside-ish
        // visual shading differences in the simple rc10 voxel renderer.
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (25, 6, 22).into(),
            max: (29, 7, 26).into(),
            block_id: ids.grass,
        });
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (26, 7, 23).into(),
            max: (29, 8, 26).into(),
            block_id: ids.dirt,
        });
        output.writes.push(WorldTerrainWrite::FillBox {
            min: (27, 8, 24).into(),
            max: (29, 9, 26).into(),
            block_id: ids.stone,
        });

        // Sparse single-block markers exercise SetBlock output and make material
        // drift obvious in screenshots.
        for (index, block_id) in [ids.stone, ids.dirt, ids.grass, ids.coarse_dirt, ids.glass]
            .into_iter()
            .enumerate()
        {
            output.writes.push(WorldTerrainWrite::SetBlock {
                pos: (4 + index as i32 * 2, 7, 24).into(),
                block_id,
            });
        }
    }
}

impl WorldGenProvider for VisualValidationWorldGen {
    fn generate(
        &mut self,
        request: &WorldGenRequest,
        output: &mut WorldGenOutput,
    ) -> Result<(), WorldGenError> {
        output.writes.clear();
        output.bootstrap.initial_world_spawn_hint = None;
        self.flat.emit_flat_column(request, output);
        self.emit_validation_scene(request, output);
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
        coarse_dirt: init
            .block_id_by_key(COARSE_DIRT_KEY)
            .expect("vanilla essentials worldgen requires resolved coarse dirt block id"),
        glass: init
            .block_id_by_key(GLASS_KEY)
            .expect("vanilla essentials worldgen requires resolved glass block id"),
    }
}

fn ensure_flat_block_ids(resolved: FlatBlockIds) {
    if let Err(existing) = FLAT_BLOCKS.set(resolved)
        && *FLAT_BLOCKS.get().expect("flat blocks must be initialized") != existing
    {
        panic!("vanilla essentials block ids must remain deterministic across runtime builds");
    }
}

#[cfg(test)]
mod worldgen_tests {
    use super::*;
    use freven_volumetric_api::ColumnCoord;

    fn test_init() -> WorldGenInit {
        let mut init = WorldGenInit::new(123);
        init.block_ids
            .insert(STONE_KEY.to_string(), BlockRuntimeId(1));
        init.block_ids
            .insert(DIRT_KEY.to_string(), BlockRuntimeId(2));
        init.block_ids
            .insert(GRASS_KEY.to_string(), BlockRuntimeId(3));
        init.block_ids
            .insert(COARSE_DIRT_KEY.to_string(), BlockRuntimeId(4));
        init.block_ids
            .insert(GLASS_KEY.to_string(), BlockRuntimeId(5));
        init
    }

    fn request(cx: i32, cz: i32) -> WorldGenRequest {
        WorldGenRequest::new(123, ColumnCoord { cx, cz })
    }

    #[test]
    fn flat_worldgen_keeps_small_default_flat_column() {
        let mut provider = FlatWorldGen::new(test_init());
        let mut output = WorldGenOutput::default();

        provider
            .generate(&request(0, 0), &mut output)
            .expect("flat worldgen should generate");

        assert_eq!(output.writes.len(), 6);
        assert!(output.bootstrap.initial_world_spawn_hint.is_none());
        assert!(
            output
                .writes
                .iter()
                .all(|write| matches!(write, WorldTerrainWrite::FillBox { .. }))
        );
    }

    #[test]
    fn visual_validation_worldgen_emits_curated_origin_scene() {
        let mut provider = VisualValidationWorldGen::new(test_init());
        let mut output = WorldGenOutput::default();

        provider
            .generate(&request(0, 0), &mut output)
            .expect("visual validation worldgen should generate");

        assert!(
            output.bootstrap.initial_world_spawn_hint.is_some(),
            "visual validation scene should provide a screenshot-friendly spawn hint"
        );
        assert!(
            output.writes.len() > 24,
            "visual validation scene should add curated material, transparency, UV, and lighting probes on top of flat terrain"
        );
        assert!(
            output
                .writes
                .iter()
                .filter(|write| matches!(write, WorldTerrainWrite::FillBox { .. }))
                .count()
                >= 20,
            "visual validation scene should mostly use FillBox probes so greedy meshing is exercised"
        );
        assert!(
            output.writes.iter().any(|write| matches!(
                write,
                WorldTerrainWrite::FillBox { block_id, .. } if *block_id == BlockRuntimeId(5)
            )),
            "visual validation scene should include transparent glass fill regions"
        );
        assert!(
            output.writes.iter().any(|write| matches!(
                write,
                WorldTerrainWrite::SetBlock { block_id, .. } if *block_id == BlockRuntimeId(4)
            )),
            "visual validation scene should include sparse coarse dirt variant markers"
        );
    }

    #[test]
    fn visual_validation_worldgen_keeps_other_columns_flat() {
        let mut provider = VisualValidationWorldGen::new(test_init());
        let mut output = WorldGenOutput::default();

        provider
            .generate(&request(1, 0), &mut output)
            .expect("visual validation worldgen should generate non-origin columns");

        assert_eq!(output.writes.len(), 6);
        assert!(output.bootstrap.initial_world_spawn_hint.is_none());
        assert!(
            output
                .writes
                .iter()
                .all(|write| matches!(write, WorldTerrainWrite::FillBox { .. }))
        );
    }
}
