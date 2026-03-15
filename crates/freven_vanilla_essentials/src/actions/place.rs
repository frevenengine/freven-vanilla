//! Handler for vanilla `freven:place` actions.

use crate::action_payloads::decode_place_payload_v1;
use freven_world_api::{ActionCmdView, ActionContext, ActionHandler, ActionOutcome};

use crate::storage_ids;

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

        if !storage_ids::is_place_allowed_v0(decoded.block_id) {
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

        let Some(world_edit) = ctx.world_edit.as_mut() else {
            return ActionOutcome::Rejected;
        };

        let hit_cur = world_edit.block_world(
            decoded.target.pos.0,
            decoded.target.pos.1,
            decoded.target.pos.2,
        );
        let target_cur = world_edit.block_world(target_pos.0, target_pos.1, target_pos.2);

        if !world_edit.is_solid_block_id(hit_cur) || storage_ids::is_solid(target_cur) {
            return ActionOutcome::Rejected;
        }

        match world_edit.try_set_block_world_if(
            target_pos.0,
            target_pos.1,
            target_pos.2,
            storage_ids::AIR_U8,
            decoded.block_id,
        ) {
            freven_world_api::ActionWorldEditResult::Applied { .. } => ActionOutcome::Applied,
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
