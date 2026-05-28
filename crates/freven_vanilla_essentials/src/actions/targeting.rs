use crate::character_controller::humanoid_config;
use freven_avatar_sdk_types::CharacterShape;

pub(crate) const MAX_ACTION_REACH_M: f32 = 5.0;
pub(crate) const PLAYER_EYE_HEIGHT_M: f32 = 1.62;

// Vanilla humanoids use an AABB with 0.9m vertical half-height.
// Keep this fallback in sync with `character_controller::humanoid_config`.
const VANILLA_HUMANOID_AABB_HALF_HEIGHT_M: f32 = 0.9;

// Server-side terrain validation must not blindly trust arbitrary client ray
// origins, but it also must not replace a captured camera/aim ray with a
// different body-center origin. This volume bounds the accepted camera/aim
// origin around the authoritative humanoid interaction origin.
const CLIENT_AIM_ORIGIN_MAX_HORIZONTAL_DRIFT_M: f32 = 0.75;
const CLIENT_AIM_ORIGIN_MAX_VERTICAL_DRIFT_M: f32 = 0.45;

pub(crate) fn humanoid_interaction_origin_m(player_center_pos_m: [f32; 3]) -> [f32; 3] {
    [
        player_center_pos_m[0],
        player_center_pos_m[1] + (PLAYER_EYE_HEIGHT_M - humanoid_half_height_m()),
        player_center_pos_m[2],
    ]
}

pub(crate) fn bounded_client_aim_origin_m(
    authoritative_interaction_origin_m: [f32; 3],
    client_ray_origin_m: [f32; 3],
) -> Option<[f32; 3]> {
    if !vec3_is_finite(authoritative_interaction_origin_m) || !vec3_is_finite(client_ray_origin_m) {
        return None;
    }

    let dx = client_ray_origin_m[0] - authoritative_interaction_origin_m[0];
    let dy = client_ray_origin_m[1] - authoritative_interaction_origin_m[1];
    let dz = client_ray_origin_m[2] - authoritative_interaction_origin_m[2];

    let horizontal_dist_sq = dx.mul_add(dx, dz * dz);
    if horizontal_dist_sq
        > CLIENT_AIM_ORIGIN_MAX_HORIZONTAL_DRIFT_M * CLIENT_AIM_ORIGIN_MAX_HORIZONTAL_DRIFT_M
    {
        return None;
    }
    if dy.abs() > CLIENT_AIM_ORIGIN_MAX_VERTICAL_DRIFT_M {
        return None;
    }

    Some(client_ray_origin_m)
}

fn vec3_is_finite(v: [f32; 3]) -> bool {
    v.into_iter().all(f32::is_finite)
}

fn humanoid_half_height_m() -> f32 {
    match humanoid_config().shape {
        CharacterShape::Aabb { half_extents } => half_extents[1],
        _ => VANILLA_HUMANOID_AABB_HALF_HEIGHT_M,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_client_aim_origin_accepts_normal_camera_lateral_offset() {
        let authoritative = [15.7, 7.6210003, 19.3];
        let client = [16.264812, 7.6210003, 19.3];

        assert_eq!(
            bounded_client_aim_origin_m(authoritative, client),
            Some(client)
        );
    }

    #[test]
    fn bounded_client_aim_origin_rejects_remote_or_non_finite_origins() {
        let authoritative = [15.7, 7.6210003, 19.3];

        assert_eq!(
            bounded_client_aim_origin_m(authoritative, [17.7, 7.6210003, 19.3]),
            None
        );
        assert_eq!(
            bounded_client_aim_origin_m(authoritative, [f32::NAN, 7.6210003, 19.3]),
            None
        );
    }
}
