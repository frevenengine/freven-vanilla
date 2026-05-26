//! Handler for vanilla `freven:break` actions.

use crate::action_payloads::decode_break_payload_v2;
use crate::actions::targeting::{MAX_ACTION_REACH_M, PLAYER_EYE_HEIGHT_M};
use freven_block_api::{
    BlockMutationResult, BlockWorldViewTerrainAdapter, TerrainInteractionRulesV2,
    TerrainInteractionValidationPolicyV2, TerrainInteractionValidationV2,
    validate_terrain_interaction_v2,
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
        let Some(player_pos) = character_physics.player_position(ctx.player_id) else {
            return ActionOutcome::Rejected;
        };

        if intent.identity.action_seq.is_some_and(|seq| seq != cmd.seq) {
            return ActionOutcome::Rejected;
        }

        let Some(block_authority) = ctx.block_authority.as_mut() else {
            return ActionOutcome::Rejected;
        };

        let policy = terrain_validation_policy(player_pos, cmd);
        let validation = {
            let world = BlockWorldViewTerrainAdapter::new(&**block_authority);
            let rules = VanillaBreakRules {
                world: &**block_authority,
            };
            validate_terrain_interaction_v2(&world, &rules, &policy, &intent)
        };

        if let TerrainInteractionValidationV2::Rejected(reason) = validation {
            emit_log(
                LogLevel::Debug,
                format!("break terrain interaction rejected: {reason:?}"),
            );
            return ActionOutcome::Rejected;
        }

        let target_pos = intent.hit.hit_block_pos;
        let Some(cur) = block_authority.block(target_pos.0, target_pos.1, target_pos.2) else {
            return ActionOutcome::Rejected;
        };

        match block_authority.try_apply(&BlockMutation::clear_block(target_pos, Some(cur))) {
            BlockMutationResult::Applied { .. } => ActionOutcome::Applied,
            _ => ActionOutcome::Rejected,
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
    player_pos: [f32; 3],
    cmd: &ActionCmdView<'_>,
) -> TerrainInteractionValidationPolicyV2 {
    let mut policy = TerrainInteractionValidationPolicyV2::new(
        [
            player_pos[0],
            player_pos[1] + PLAYER_EYE_HEIGHT_M,
            player_pos[2],
        ],
        MAX_ACTION_REACH_M,
    );
    policy.active_level_id = Some(cmd.level_id);
    policy.active_stream_epoch = Some(cmd.stream_epoch);
    policy.min_input_seq = Some(cmd.at_input_seq);
    policy.max_input_seq = Some(cmd.at_input_seq);
    policy
}
