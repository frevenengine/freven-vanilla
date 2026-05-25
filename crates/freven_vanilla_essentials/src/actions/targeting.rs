use freven_block_api::BlockWorldView;

pub(crate) const MAX_ACTION_REACH_M: f32 = 5.0;
pub(crate) const MAX_COORD_ABS: i32 = 2_000_000;
pub(crate) const PLAYER_EYE_HEIGHT_M: f32 = 1.62;

#[inline]
pub(crate) fn is_sane_pos(pos: (i32, i32, i32)) -> bool {
    pos.0.abs() <= MAX_COORD_ABS && pos.1.abs() <= MAX_COORD_ABS && pos.2.abs() <= MAX_COORD_ABS
}

#[inline]
pub(crate) fn target_from_face(hit: (i32, i32, i32), face: u8) -> Option<(i32, i32, i32)> {
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
pub(crate) fn within_reach(
    player_pos: [f32; 3],
    target: (i32, i32, i32),
    max_distance_m: f32,
) -> bool {
    let [tx, ty, tz] = block_center(target);
    let dx = player_pos[0] - tx;
    let dy = player_pos[1] - ty;
    let dz = player_pos[2] - tz;
    (dx * dx + dy * dy + dz * dz) <= max_distance_m * max_distance_m
}

pub(crate) fn first_solid_target_visible(
    world: &dyn BlockWorldView,
    player_pos: [f32; 3],
    target: (i32, i32, i32),
    max_distance_m: f32,
) -> bool {
    let origin = [
        player_pos[0],
        player_pos[1] + PLAYER_EYE_HEIGHT_M,
        player_pos[2],
    ];
    let target_center = block_center(target);
    let dx = target_center[0] - origin[0];
    let dy = target_center[1] - origin[1];
    let dz = target_center[2] - origin[2];
    let len_sq = dx * dx + dy * dy + dz * dz;

    if len_sq <= f32::EPSILON {
        return false;
    }

    let len = len_sq.sqrt();
    if len > max_distance_m {
        return false;
    }

    let dir = [dx / len, dy / len, dz / len];
    first_solid_along_ray(world, origin, dir, len + 0.05).is_some_and(|hit| hit == target)
}

#[inline]
fn block_center(pos: (i32, i32, i32)) -> [f32; 3] {
    [pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5]
}

fn first_solid_along_ray(
    world: &dyn BlockWorldView,
    origin: [f32; 3],
    dir: [f32; 3],
    max_dist: f32,
) -> Option<(i32, i32, i32)> {
    let ox = origin[0] + dir[0] * 1.0e-4;
    let oy = origin[1] + dir[1] * 1.0e-4;
    let oz = origin[2] + dir[2] * 1.0e-4;

    let mut vx = ox.floor() as i32;
    let mut vy = oy.floor() as i32;
    let mut vz = oz.floor() as i32;

    let step_x = if dir[0] > 0.0 { 1 } else { -1 };
    let step_y = if dir[1] > 0.0 { 1 } else { -1 };
    let step_z = if dir[2] > 0.0 { 1 } else { -1 };

    let inv_x = if dir[0].abs() > f32::EPSILON {
        1.0 / dir[0].abs()
    } else {
        f32::INFINITY
    };
    let inv_y = if dir[1].abs() > f32::EPSILON {
        1.0 / dir[1].abs()
    } else {
        f32::INFINITY
    };
    let inv_z = if dir[2].abs() > f32::EPSILON {
        1.0 / dir[2].abs()
    } else {
        f32::INFINITY
    };

    let next_boundary_x = if step_x > 0 {
        vx as f32 + 1.0
    } else {
        vx as f32
    };
    let next_boundary_y = if step_y > 0 {
        vy as f32 + 1.0
    } else {
        vy as f32
    };
    let next_boundary_z = if step_z > 0 {
        vz as f32 + 1.0
    } else {
        vz as f32
    };

    let mut t_max_x = if dir[0].abs() > f32::EPSILON {
        (next_boundary_x - ox) / dir[0]
    } else {
        f32::INFINITY
    };
    let mut t_max_y = if dir[1].abs() > f32::EPSILON {
        (next_boundary_y - oy) / dir[1]
    } else {
        f32::INFINITY
    };
    let mut t_max_z = if dir[2].abs() > f32::EPSILON {
        (next_boundary_z - oz) / dir[2]
    } else {
        f32::INFINITY
    };

    for _ in 0..256 {
        let block = world.block(vx, vy, vz)?;
        if world.is_solid(block) {
            return Some((vx, vy, vz));
        }

        let traveled;
        if t_max_x <= t_max_y && t_max_x <= t_max_z {
            traveled = t_max_x;
            vx += step_x;
            t_max_x += inv_x;
        } else if t_max_y <= t_max_z {
            traveled = t_max_y;
            vy += step_y;
            t_max_y += inv_y;
        } else {
            traveled = t_max_z;
            vz += step_z;
            t_max_z += inv_z;
        }

        if traveled > max_dist {
            return None;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use freven_block_sdk_types::BlockRuntimeId;
    use std::collections::HashMap;

    #[derive(Default)]
    struct TestWorld {
        blocks: HashMap<(i32, i32, i32), BlockRuntimeId>,
    }

    impl TestWorld {
        fn with_block(mut self, pos: (i32, i32, i32), id: u32) -> Self {
            self.blocks.insert(pos, BlockRuntimeId(id));
            self
        }
    }

    impl BlockWorldView for TestWorld {
        fn block(&self, wx: i32, wy: i32, wz: i32) -> Option<BlockRuntimeId> {
            Some(*self.blocks.get(&(wx, wy, wz)).unwrap_or(&BlockRuntimeId(0)))
        }

        fn is_solid(&self, block_id: BlockRuntimeId) -> bool {
            block_id.0 != 0
        }
    }

    #[test]
    fn first_solid_target_visible_accepts_clear_target() {
        let world = TestWorld::default().with_block((4, 2, 0), 1);
        assert!(first_solid_target_visible(
            &world,
            [0.5, 0.0, 0.5],
            (4, 2, 0),
            MAX_ACTION_REACH_M,
        ));
    }

    #[test]
    fn first_solid_target_visible_rejects_occluded_target() {
        let world = TestWorld::default()
            .with_block((2, 2, 0), 1)
            .with_block((4, 2, 0), 1);
        assert!(!first_solid_target_visible(
            &world,
            [0.5, 0.0, 0.5],
            (4, 2, 0),
            MAX_ACTION_REACH_M,
        ));
    }

    #[test]
    fn first_solid_target_visible_rejects_out_of_reach_target() {
        let world = TestWorld::default().with_block((12, 2, 0), 1);
        assert!(!first_solid_target_visible(
            &world,
            [0.5, 0.0, 0.5],
            (12, 2, 0),
            MAX_ACTION_REACH_M,
        ));
    }
}
