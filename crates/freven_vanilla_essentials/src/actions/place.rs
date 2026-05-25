//! Handler for vanilla `freven:place` actions.

use crate::STONE_KEY;
use crate::action_payloads::decode_place_payload_v1;
use crate::actions::targeting::{
    MAX_ACTION_REACH_M, first_solid_target_visible, is_sane_pos, target_from_face, within_reach,
};
use freven_block_api::BlockMutationResult;
use freven_block_guest::BlockMutation;
use freven_block_sdk_types::BlockRuntimeId;
use freven_world_api::{ActionCmdView, ActionContext, ActionHandler, ActionOutcome};

#[derive(Debug, Default)]
pub struct PlaceActionHandler;

impl ActionHandler for PlaceActionHandler {
    fn handle(&mut self, ctx: &mut ActionContext<'_>, cmd: &ActionCmdView<'_>) -> ActionOutcome {
        let Ok(decoded) = decode_place_payload_v1(cmd.payload) else {
            return ActionOutcome::Rejected;
        };

        if decoded.target.face > 5 || !is_sane_pos(decoded.target.pos) {
            return ActionOutcome::Rejected;
        }

        let Some(stone) = ctx.block_id_by_key(STONE_KEY) else {
            return ActionOutcome::Rejected;
        };
        let Ok(stone_wire_id) = u8::try_from(stone.0) else {
            return ActionOutcome::Rejected;
        };
        if decoded.block_id != stone_wire_id {
            return ActionOutcome::Rejected;
        }

        let Some(target_pos) = target_from_face(decoded.target.pos, decoded.target.face) else {
            return ActionOutcome::Rejected;
        };

        if !is_sane_pos(target_pos) {
            return ActionOutcome::Rejected;
        }

        let Some(character_physics) = ctx.character_physics else {
            return ActionOutcome::Rejected;
        };
        let Some(player_pos) = character_physics.player_position(ctx.player_id) else {
            return ActionOutcome::Rejected;
        };

        if !within_reach(player_pos, decoded.target.pos, MAX_ACTION_REACH_M) {
            return ActionOutcome::Rejected;
        }

        let Some(block_authority) = ctx.block_authority.as_mut() else {
            return ActionOutcome::Rejected;
        };

        let Some(hit_cur) = block_authority.block(
            decoded.target.pos.0,
            decoded.target.pos.1,
            decoded.target.pos.2,
        ) else {
            return ActionOutcome::Rejected;
        };

        let Some(target_cur) = block_authority.block(target_pos.0, target_pos.1, target_pos.2)
        else {
            return ActionOutcome::Rejected;
        };

        if !block_authority.is_solid(hit_cur) || block_authority.is_solid(target_cur) {
            return ActionOutcome::Rejected;
        }

        if !first_solid_target_visible(
            *block_authority,
            player_pos,
            decoded.target.pos,
            MAX_ACTION_REACH_M,
        ) {
            return ActionOutcome::Rejected;
        }

        match block_authority.try_apply(&BlockMutation::SetBlock {
            pos: target_pos,
            block_id: BlockRuntimeId(u32::from(decoded.block_id)),
            expected_old: Some(target_cur),
        }) {
            BlockMutationResult::Applied { .. } => ActionOutcome::Applied,
            _ => ActionOutcome::Rejected,
        }
    }
}
