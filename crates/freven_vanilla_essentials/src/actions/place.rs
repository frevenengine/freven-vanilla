//! Handler for vanilla `freven:place` actions.

use crate::STONE_KEY;
use crate::action_payloads::decode_place_payload_v2;
use crate::actions::r#break::terrain_validation_policy;
use crate::actions::targeting::humanoid_interaction_origin_m;
use freven_block_api::{
    BlockMutationResult, BlockWorldViewTerrainAdapter, ClientBlockFace, TerrainInteractionRulesV2,
    TerrainInteractionValidationV2, validate_terrain_interaction_v2,
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
            validate_terrain_interaction_v2(&world, &rules, &policy, &intent)
        };

        if let TerrainInteractionValidationV2::Rejected(reason) = validation {
            emit_log(
                LogLevel::Debug,
                format!(
                    "place terrain interaction rejected: reason={reason:?} player_id={} \
                     authoritative_origin_m={:?} client_ray_origin_m={:?} ray_dir={:?} \
                     hit_block_pos={:?} hit_face={:?} placement_pos={:?}",
                    ctx.player_id,
                    authoritative_origin_m,
                    intent.ray.ray_origin_m,
                    intent.ray.ray_dir,
                    intent.hit.hit_block_pos,
                    intent.hit.hit_face,
                    intent.place.as_ref().map(|place| place.placement_pos),
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
            return ActionOutcome::Rejected;
        };

        match block_authority.try_apply(&BlockMutation::SetBlock {
            pos: target_pos,
            block_id: place.block_id,
            expected_old: Some(target_cur),
        }) {
            BlockMutationResult::Applied { .. } => ActionOutcome::Applied,
            _ => ActionOutcome::Rejected,
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
