//! Handler for vanilla `freven:place` actions.

use crate::STONE_KEY;
use crate::action_payloads::decode_place_payload_v2;
use crate::actions::r#break::terrain_validation_policy;
use crate::actions::targeting::humanoid_interaction_origin_m;
use freven_block_api::{
    BlockMutationResult, BlockWorldViewTerrainAdapter, ClientBlockFace, TerrainInteractionRulesV2,
    TerrainInteractionValidationV2, validate_terrain_interaction_v2_report,
};
use freven_block_guest::BlockMutation;
use freven_block_sdk_types::BlockRuntimeId;
use freven_mod_api::{LogLevel, emit_log};
use freven_world_api::{ActionCmdView, ActionContext, ActionHandler, ActionOutcome};

#[derive(Debug, Default)]
pub struct PlaceActionHandler;

impl ActionHandler for PlaceActionHandler {
    fn handle(&mut self, ctx: &mut ActionContext<'_>, cmd: &ActionCmdView<'_>) -> ActionOutcome {
        let Ok(intent) = decode_place_payload_v2(cmd.payload) else {
            return ActionOutcome::Rejected;
        };

        if intent.identity.action_seq.is_some_and(|seq| seq != cmd.seq) {
            return ActionOutcome::Rejected;
        }

        let Some(stone) = ctx.block_id_by_key(STONE_KEY) else {
            return ActionOutcome::Rejected;
        };

        let Some(character_physics) = ctx.character_physics else {
            return ActionOutcome::Rejected;
        };
        let Some(player_center_pos) = character_physics.player_position(ctx.player_id) else {
            return ActionOutcome::Rejected;
        };

        let Some(block_authority) = ctx.block_authority.as_mut() else {
            return ActionOutcome::Rejected;
        };

        let authoritative_origin_m = humanoid_interaction_origin_m(player_center_pos);
        let policy = terrain_validation_policy(player_center_pos, cmd);
        let validation = {
            let world = BlockWorldViewTerrainAdapter::new(&**block_authority);
            let rules = VanillaPlaceRules {
                world: &**block_authority,
                allowed_block_id: stone,
            };
            validate_terrain_interaction_v2_report(&world, &rules, &policy, &intent)
        };

        if let TerrainInteractionValidationV2::Rejected(reason) = validation.validation {
            let target_pos = intent.hit.hit_block_pos;
            let place_pos = intent.place.as_ref().map(|place| place.placement_pos);
            let target_block = block_authority.block(target_pos.0, target_pos.1, target_pos.2);
            let place_block = place_pos.and_then(|pos| block_authority.block(pos.0, pos.1, pos.2));
            tracing::debug!(
                target: "freven_vanilla_essentials::actions::place",
                player_id = ctx.player_id,
                action_seq = cmd.seq,
                intent_action_seq = ?intent.identity.action_seq,
                at_input_seq = cmd.at_input_seq,
                reason = ?reason,
                target_pos = ?target_pos,
                place_pos = ?place_pos,
                hit_face = ?intent.hit.hit_face,
                ray_origin_m = ?intent.ray.ray_origin_m,
                ray_dir = ?intent.ray.ray_dir,
                authoritative_target_block = ?target_block,
                authoritative_place_block = ?place_block,
                server_interaction_origin_m = ?authoritative_origin_m,
                trace = ?validation.trace,
                "terrain interaction rejected",
            );
            emit_log(
                LogLevel::Debug,
                format!(
                    "terrain interaction rejected: kind=place reason={reason:?} player_id={} \
                     action_seq={} intent_action_seq={:?} at_input_seq={} target_pos={:?} \
                     hit_block_pos={:?} place_pos={:?} hit_face={:?} ray_origin_m={:?} \
                     ray_dir={:?} authoritative_target_block={:?} \
                     authoritative_place_block={:?} server_interaction_origin_m={:?} trace={:?}",
                    ctx.player_id,
                    cmd.seq,
                    intent.identity.action_seq,
                    cmd.at_input_seq,
                    target_pos,
                    intent.hit.hit_block_pos,
                    place_pos,
                    intent.hit.hit_face,
                    intent.ray.ray_origin_m,
                    intent.ray.ray_dir,
                    target_block,
                    place_block,
                    authoritative_origin_m,
                    validation.trace,
                ),
            );
            return ActionOutcome::Rejected;
        }

        let Some(place) = intent.place else {
            return ActionOutcome::Rejected;
        };
        let target_pos = place.placement_pos;

        let Some(target_cur) = block_authority.block(target_pos.0, target_pos.1, target_pos.2)
        else {
            tracing::debug!(
                target: "freven_vanilla_essentials::actions::place",
                player_id = ctx.player_id,
                action_seq = cmd.seq,
                intent_action_seq = ?intent.identity.action_seq,
                at_input_seq = cmd.at_input_seq,
                target_pos = ?target_pos,
                block_id = ?place.block_id,
                "place action rejected: placement block missing after validation",
            );
            emit_log(
                LogLevel::Debug,
                format!(
                    "place action rejected: placement block missing after validation player_id={}                      action_seq={} intent_action_seq={:?} at_input_seq={} target_pos={:?} block_id={:?}",
                    ctx.player_id,
                    cmd.seq,
                    intent.identity.action_seq,
                    cmd.at_input_seq,
                    target_pos,
                    place.block_id,
                ),
            );
            return ActionOutcome::Rejected;
        };

        let mutation_result = block_authority.try_apply(&BlockMutation::SetBlock {
            pos: target_pos,
            block_id: place.block_id,
            expected_old: Some(target_cur),
        });

        match mutation_result {
            BlockMutationResult::Applied { .. } => ActionOutcome::Applied,
            result => {
                tracing::debug!(
                    target: "freven_vanilla_essentials::actions::place",
                    player_id = ctx.player_id,
                    action_seq = cmd.seq,
                    intent_action_seq = ?intent.identity.action_seq,
                    at_input_seq = cmd.at_input_seq,
                    target_pos = ?target_pos,
                    block_id = ?place.block_id,
                    expected_old = ?target_cur,
                    mutation_result = ?result,
                    "place action rejected: mutation apply failed",
                );
                emit_log(
                    LogLevel::Debug,
                    format!(
                        "place action rejected: mutation apply failed player_id={} action_seq={}                          intent_action_seq={:?} at_input_seq={} target_pos={:?} block_id={:?}                          expected_old={:?} mutation_result={:?}",
                        ctx.player_id,
                        cmd.seq,
                        intent.identity.action_seq,
                        cmd.at_input_seq,
                        target_pos,
                        place.block_id,
                        target_cur,
                        result,
                    ),
                );
                ActionOutcome::Rejected
            }
        }
    }
}

struct VanillaPlaceRules<'a> {
    world: &'a dyn freven_block_api::BlockWorldView,
    allowed_block_id: BlockRuntimeId,
}

impl TerrainInteractionRulesV2 for VanillaPlaceRules<'_> {
    fn is_solid(&self, block_id: BlockRuntimeId) -> bool {
        self.world.is_solid(block_id)
    }

    fn is_supporting(&self, block_id: BlockRuntimeId, _face: ClientBlockFace) -> bool {
        self.world.is_solid(block_id)
    }

    fn can_place_block(&self, block_id: BlockRuntimeId) -> bool {
        block_id == self.allowed_block_id
    }
}
