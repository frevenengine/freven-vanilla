//! Handler for vanilla `freven:break` actions.

use crate::action_payloads::decode_break_payload_v1;
use crate::actions::targeting::{
    MAX_ACTION_REACH_M, first_solid_target_visible, is_sane_pos, within_reach,
};
use freven_block_api::BlockMutationResult;
use freven_block_guest::BlockMutation;
use freven_world_api::{ActionCmdView, ActionContext, ActionHandler, ActionOutcome};

const BREAK_STATUS_FINISHED: u8 = 2;

#[derive(Debug, Default)]
pub struct BreakActionHandler;

impl ActionHandler for BreakActionHandler {
    fn handle(&mut self, ctx: &mut ActionContext<'_>, cmd: &ActionCmdView<'_>) -> ActionOutcome {
        let Ok(decoded) = decode_break_payload_v1(cmd.payload) else {
            return ActionOutcome::Rejected;
        };

        if decoded.status != BREAK_STATUS_FINISHED {
            return ActionOutcome::Rejected;
        }

        if decoded.target.face > 5 || !is_sane_pos(decoded.target.pos) {
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

        let Some(cur) = block_authority.block(
            decoded.target.pos.0,
            decoded.target.pos.1,
            decoded.target.pos.2,
        ) else {
            return ActionOutcome::Rejected;
        };

        if !block_authority.is_solid(cur) {
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

        match block_authority.try_apply(&BlockMutation::clear_block(decoded.target.pos, Some(cur)))
        {
            BlockMutationResult::Applied { .. } => ActionOutcome::Applied,
            _ => ActionOutcome::Rejected,
        }
    }
}
