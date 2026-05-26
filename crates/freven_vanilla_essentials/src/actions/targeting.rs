use crate::character_controller::humanoid_config;
use freven_avatar_sdk_types::CharacterShape;

pub(crate) const MAX_ACTION_REACH_M: f32 = 5.0;
pub(crate) const PLAYER_EYE_HEIGHT_M: f32 = 1.62;

// Vanilla humanoids use an AABB with 0.9m vertical half-height.
// Keep this fallback in sync with `character_controller::humanoid_config`.
const VANILLA_HUMANOID_AABB_HALF_HEIGHT_M: f32 = 0.9;

pub(crate) fn humanoid_interaction_origin_m(player_center_pos_m: [f32; 3]) -> [f32; 3] {
    [
        player_center_pos_m[0],
        player_center_pos_m[1] + (PLAYER_EYE_HEIGHT_M - humanoid_half_height_m()),
        player_center_pos_m[2],
    ]
}

fn humanoid_half_height_m() -> f32 {
    match humanoid_config().shape {
        CharacterShape::Aabb { half_extents } => half_extents[1],
        _ => VANILLA_HUMANOID_AABB_HALF_HEIGHT_M,
    }
}
