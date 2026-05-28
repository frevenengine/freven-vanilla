//! Handler for vanilla `freven:break` actions.

use crate::action_payloads::decode_break_payload_v2;
use crate::actions::targeting::{
    MAX_ACTION_REACH_M, bounded_client_aim_origin_m, humanoid_interaction_origin_m,
};
use freven_block_api::{
    BlockMutationResult, BlockWorldViewTerrainAdapter, TerrainInteractionRejectReasonV2,
    TerrainInteractionRulesV2, TerrainInteractionValidationPolicyV2,
    TerrainInteractionValidationV2, validate_terrain_interaction_v2_report,
};
use freven_block_guest::BlockMutation;
use freven_block_sdk_types::BlockRuntimeId;
use freven_mod_api::{LogLevel, emit_log};
use freven_world_api::{ActionCmdView, ActionContext, ActionHandler, ActionOutcome};

#[derive(Debug, Default)]
pub struct BreakActionHandler;

impl ActionHandler for BreakActionHandler {
    fn handle(&mut self, ctx: &mut ActionContext<'_>, cmd: &ActionCmdView<'_>) -> ActionOutcome {
        let Ok(intent) = decode_break_payload_v2(cmd.payload) else {
            return ActionOutcome::Rejected;
        };

        let Some(character_physics) = ctx.character_physics else {
            return ActionOutcome::Rejected;
        };
        let Some(player_center_pos) = character_physics.player_position(ctx.player_id) else {
            return ActionOutcome::Rejected;
        };

        if intent.identity.action_seq.is_some_and(|seq| seq != cmd.seq) {
            return ActionOutcome::Rejected;
        }

        let Some(block_authority) = ctx.block_authority.as_mut() else {
            return ActionOutcome::Rejected;
        };

        let authoritative_origin_m = humanoid_interaction_origin_m(player_center_pos);
        let Some(validation_origin_m) =
            bounded_client_aim_origin_m(authoritative_origin_m, intent.ray.ray_origin_m)
        else {
            let target_pos = intent.hit.hit_block_pos;
            tracing::debug!(
                target: "freven_vanilla_essentials::actions::break",
                player_id = ctx.player_id,
                action_seq = cmd.seq,
                intent_action_seq = ?intent.identity.action_seq,
                at_input_seq = cmd.at_input_seq,
                reason = ?TerrainInteractionRejectReasonV2::PolicyDenied,
                target_pos = ?target_pos,
                hit_face = ?intent.hit.hit_face,
                ray_origin_m = ?intent.ray.ray_origin_m,
                ray_dir = ?intent.ray.ray_dir,
                authoritative_humanoid_origin_m = ?authoritative_origin_m,
                "terrain interaction rejected",
            );
            emit_log(
                LogLevel::Debug,
                format!(
                    "terrain interaction rejected: kind=break reason={:?} player_id={}                      action_seq={} intent_action_seq={:?} at_input_seq={} target_pos={:?}                      hit_block_pos={:?} place_pos=None hit_face={:?} ray_origin_m={:?}                      ray_dir={:?} authoritative_humanoid_origin_m={:?}",
                    TerrainInteractionRejectReasonV2::PolicyDenied,
                    ctx.player_id,
                    cmd.seq,
                    intent.identity.action_seq,
                    cmd.at_input_seq,
                    target_pos,
                    intent.hit.hit_block_pos,
                    intent.hit.hit_face,
                    intent.ray.ray_origin_m,
                    intent.ray.ray_dir,
                    authoritative_origin_m,
                ),
            );
            return ActionOutcome::Rejected;
        };

        let policy = terrain_validation_policy(validation_origin_m, cmd);
        let validation = {
            let world = BlockWorldViewTerrainAdapter::new(&**block_authority);
            let rules = VanillaBreakRules {
                world: &**block_authority,
            };
            validate_terrain_interaction_v2_report(&world, &rules, &policy, &intent)
        };

        if let TerrainInteractionValidationV2::Rejected(reason) = validation.validation {
            let target_pos = intent.hit.hit_block_pos;
            let target_block = block_authority.block(target_pos.0, target_pos.1, target_pos.2);
            tracing::debug!(
                target: "freven_vanilla_essentials::actions::break",
                player_id = ctx.player_id,
                action_seq = cmd.seq,
                intent_action_seq = ?intent.identity.action_seq,
                at_input_seq = cmd.at_input_seq,
                reason = ?reason,
                target_pos = ?target_pos,
                hit_face = ?intent.hit.hit_face,
                ray_origin_m = ?intent.ray.ray_origin_m,
                ray_dir = ?intent.ray.ray_dir,
                authoritative_target_block = ?target_block,
                server_interaction_origin_m = ?validation_origin_m,
                authoritative_humanoid_origin_m = ?authoritative_origin_m,
                trace = ?validation.trace,
                "terrain interaction rejected",
            );
            emit_log(
                LogLevel::Debug,
                format!(
                    "terrain interaction rejected: kind=break reason={reason:?} player_id={} \
                     action_seq={} intent_action_seq={:?} at_input_seq={} target_pos={:?} \
                     hit_block_pos={:?} place_pos=None hit_face={:?} ray_origin_m={:?} \
                     ray_dir={:?} authoritative_target_block={:?} \
                     authoritative_place_block=None server_interaction_origin_m={:?} authoritative_humanoid_origin_m={:?} trace={:?}",
                    ctx.player_id,
                    cmd.seq,
                    intent.identity.action_seq,
                    cmd.at_input_seq,
                    target_pos,
                    intent.hit.hit_block_pos,
                    intent.hit.hit_face,
                    intent.ray.ray_origin_m,
                    intent.ray.ray_dir,
                    target_block,
                    validation_origin_m,
                    authoritative_origin_m,
                    validation.trace,
                ),
            );
            return ActionOutcome::Rejected;
        }

        let target_pos = intent.hit.hit_block_pos;
        let Some(cur) = block_authority.block(target_pos.0, target_pos.1, target_pos.2) else {
            tracing::debug!(
                target: "freven_vanilla_essentials::actions::break",
                player_id = ctx.player_id,
                action_seq = cmd.seq,
                intent_action_seq = ?intent.identity.action_seq,
                at_input_seq = cmd.at_input_seq,
                target_pos = ?target_pos,
                "break action rejected: target block missing after validation",
            );
            emit_log(
                LogLevel::Debug,
                format!(
                    "break action rejected: target block missing after validation player_id={}                      action_seq={} intent_action_seq={:?} at_input_seq={} target_pos={:?}",
                    ctx.player_id,
                    cmd.seq,
                    intent.identity.action_seq,
                    cmd.at_input_seq,
                    target_pos,
                ),
            );
            return ActionOutcome::Rejected;
        };

        let mutation_result =
            block_authority.try_apply(&BlockMutation::clear_block(target_pos, Some(cur)));

        match mutation_result {
            BlockMutationResult::Applied { .. } => ActionOutcome::Applied,
            result => {
                tracing::debug!(
                    target: "freven_vanilla_essentials::actions::break",
                    player_id = ctx.player_id,
                    action_seq = cmd.seq,
                    intent_action_seq = ?intent.identity.action_seq,
                    at_input_seq = cmd.at_input_seq,
                    target_pos = ?target_pos,
                    expected_old = ?cur,
                    mutation_result = ?result,
                    "break action rejected: mutation apply failed",
                );
                emit_log(
                    LogLevel::Debug,
                    format!(
                        "break action rejected: mutation apply failed player_id={} action_seq={}                          intent_action_seq={:?} at_input_seq={} target_pos={:?} expected_old={:?}                          mutation_result={:?}",
                        ctx.player_id,
                        cmd.seq,
                        intent.identity.action_seq,
                        cmd.at_input_seq,
                        target_pos,
                        cur,
                        result,
                    ),
                );
                ActionOutcome::Rejected
            }
        }
    }
}

struct VanillaBreakRules<'a> {
    world: &'a dyn freven_block_api::BlockWorldView,
}

impl TerrainInteractionRulesV2 for VanillaBreakRules<'_> {
    fn is_solid(&self, block_id: BlockRuntimeId) -> bool {
        self.world.is_solid(block_id)
    }
}

pub(crate) fn terrain_validation_policy(
    interaction_origin_m: [f32; 3],
    cmd: &ActionCmdView<'_>,
) -> TerrainInteractionValidationPolicyV2 {
    let mut policy =
        TerrainInteractionValidationPolicyV2::new(interaction_origin_m, MAX_ACTION_REACH_M);
    policy.active_level_id = Some(cmd.level_id);
    policy.active_stream_epoch = Some(cmd.stream_epoch);
    policy.min_input_seq = Some(cmd.at_input_seq);
    policy.max_input_seq = Some(cmd.at_input_seq);
    policy
}
