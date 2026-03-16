//! Handler for vanilla `freven:break` actions.

use crate::action_payloads::decode_break_payload_v1;
use freven_world_api::{
    ActionCmdView, ActionContext, ActionHandler, ActionOutcome, WorldMutation, WorldMutationResult,
};

const MAX_ACTION_REACH_M: f32 = 5.0;
const MAX_COORD_ABS: i32 = 2_000_000;
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
        let Some(world_edit) = ctx.authority.as_mut() else {
            return ActionOutcome::Rejected;
        };

        let Some(cur) = world_edit.block(
            decoded.target.pos.0,
            decoded.target.pos.1,
            decoded.target.pos.2,
        ) else {
            return ActionOutcome::Rejected;
        };

        if !world_edit.is_solid(cur) {
            return ActionOutcome::Rejected;
        }
        match world_edit.try_apply(&WorldMutation::clear_block(decoded.target.pos, Some(cur))) {
            WorldMutationResult::Applied { .. } => ActionOutcome::Applied,
            _ => ActionOutcome::Rejected,
        }
    }
}

#[inline]
fn is_sane_pos(pos: (i32, i32, i32)) -> bool {
    pos.0.abs() <= MAX_COORD_ABS && pos.1.abs() <= MAX_COORD_ABS && pos.2.abs() <= MAX_COORD_ABS
}

#[inline]
fn within_reach(player_pos: [f32; 3], target: (i32, i32, i32), max_distance_m: f32) -> bool {
    let cx = target.0 as f32 + 0.5;
    let cy = target.1 as f32 + 0.5;
    let cz = target.2 as f32 + 0.5;
    let dx = player_pos[0] - cx;
    let dy = player_pos[1] - cy;
    let dz = player_pos[2] - cz;
    (dx * dx + dy * dy + dz * dz) <= max_distance_m * max_distance_m
}
