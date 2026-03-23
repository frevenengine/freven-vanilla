//! Handler for vanilla `freven:place` actions.

use crate::STONE_KEY;
use crate::action_payloads::decode_place_payload_v1;
use freven_block_api::BlockMutationResult;
use freven_block_guest::BlockMutation;
use freven_block_sdk_types::BlockRuntimeId;
use freven_world_api::{ActionCmdView, ActionContext, ActionHandler, ActionOutcome};

const MAX_ACTION_REACH_M: f32 = 5.0;
const MAX_COORD_ABS: i32 = 2_000_000;

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

#[inline]
fn is_sane_pos(pos: (i32, i32, i32)) -> bool {
    pos.0.abs() <= MAX_COORD_ABS && pos.1.abs() <= MAX_COORD_ABS && pos.2.abs() <= MAX_COORD_ABS
}

#[inline]
fn target_from_face(hit: (i32, i32, i32), face: u8) -> Option<(i32, i32, i32)> {
    let delta = match face {
        0 => (-1, 0, 0),
        1 => (1, 0, 0),
        2 => (0, -1, 0),
        3 => (0, 1, 0),
        4 => (0, 0, -1),
        5 => (0, 0, 1),
        _ => return None,
    };
    Some((
        hit.0.checked_add(delta.0)?,
        hit.1.checked_add(delta.1)?,
        hit.2.checked_add(delta.2)?,
    ))
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
