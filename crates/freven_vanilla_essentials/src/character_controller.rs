//! Vanilla humanoid character controller implementation.
//!
//! Responsibilities:
//! - interpret raw button/axis input into semantic movement intent
//! - apply deterministic walk/jump/fall kinematic stepping
//! - query collisions only through `freven_avatar_sdk_types::CharacterPhysics`
//!
//! Collision model:
//! - terrain collision is delegated to `move_aabb_terrain` (VS-style push-out)
//! - controller integrates velocity and consumes applied motion/collision flags
//! - no controller-side sweep TOI, probe snapping, or depenetration loops

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::humanoid_input::{button_bits, decode_humanoid_input_v1};
use freven_avatar_sdk_types::{
    CharacterConfig, CharacterController, CharacterControllerInit, CharacterControllerInput,
    CharacterPhysics, CharacterShape, CharacterState, KinematicMoveConfig,
};
use freven_mod_api::LogLevel;

const HUMANOID_SPRINT_MULTIPLIER: f32 = 1.5;

const SKIN_MIN: f32 = 1.0e-4;
const SKIN_MAX: f32 = 2.0e-2;
const CONTACT_EPSILON: f32 = 1.0e-4;
const GROUND_PROBE_MAX: f32 = 0.05; // 5cm: small adhesion/floor probe budget

pub const HUMANOID_KEY: &str = "freven.vanilla:humanoid";

pub fn humanoid_factory(_init: CharacterControllerInit) -> Box<dyn CharacterController> {
    Box::new(HumanoidController::new())
}

#[derive(Debug, Clone)]
pub struct HumanoidController {
    config: CharacterConfig,
}

impl HumanoidController {
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: humanoid_config(),
        }
    }
}

impl Default for HumanoidController {
    fn default() -> Self {
        Self::new()
    }
}

impl CharacterController for HumanoidController {
    fn config(&self) -> &CharacterConfig {
        &self.config
    }

    fn step(
        &mut self,
        state: &mut CharacterState,
        input: &CharacterControllerInput,
        physics: &mut dyn CharacterPhysics,
        dt: Duration,
    ) {
        let intent = decode_intent(input);
        let Some(half_extents) = shape_half_extents(self.config.shape) else {
            return;
        };

        let skin = self.config.skin_width.abs().clamp(SKIN_MIN, SKIN_MAX);
        let move_cfg = KinematicMoveConfig {
            skin_width: skin,
            contact_epsilon: CONTACT_EPSILON,
            ..KinematicMoveConfig::default()
        }
        .validated();

        let dt_s = dt.as_secs_f32().max(0.0);
        if dt_s <= f32::EPSILON {
            let out = physics.move_aabb_terrain(half_extents, state.pos, [0.0, 0.0, 0.0], move_cfg);
            state.pos = out.pos;
            if state.on_ground && state.vel[1] < 0.0 {
                state.vel[1] = 0.0;
            }
            return;
        }

        let mut vel = state.vel;
        let was_grounded = state.on_ground;

        let (wish_world_x, wish_world_z) = wish_dir_world(&intent);

        let ground_speed = if intent.sprint {
            self.config.max_speed_ground * HUMANOID_SPRINT_MULTIPLIER
        } else {
            self.config.max_speed_ground
        };

        let max_speed = if was_grounded {
            ground_speed
        } else {
            self.config.max_speed_air
        }
        .max(0.0);

        let accel = if was_grounded {
            self.config.accel_ground
        } else {
            self.config.accel_air
        }
        .max(0.0);

        let desired_vx = wish_world_x * max_speed;
        let desired_vz = wish_world_z * max_speed;
        accelerate_horiz(&mut vel, desired_vx, desired_vz, accel, dt_s);

        let jumped_this_tick = was_grounded && intent.jump;
        if jumped_this_tick {
            vel[1] = self.config.jump_impulse.max(0.0);
        } else {
            vel[1] -= self.config.gravity.max(0.0) * dt_s;
        }

        let motion = [vel[0] * dt_s, vel[1] * dt_s, vel[2] * dt_s];
        let out = physics.move_aabb_terrain(half_extents, state.pos, motion, move_cfg);

        let mut pos = out.pos;
        let mut applied = out.applied_motion;

        let mut hit_x = out.hit_x;
        let mut hit_y = out.hit_y;
        let mut hit_z = out.hit_z;

        // "Grounded" is a state, not "did we hit down this tick".
        let mut on_ground = out.hit_ground;

        // Floor probe: keep grounded stable even when vertical motion is ~0.
        // Mirrors big engines: separate floor test.
        if !on_ground && was_grounded && !jumped_this_tick && vel[1] <= 0.0 {
            let probe_dist = (skin * 2.0).clamp(CONTACT_EPSILON * 4.0, GROUND_PROBE_MAX);

            if probe_dist > f32::EPSILON {
                let probe =
                    physics.move_aabb_terrain(half_extents, pos, [0.0, -probe_dist, 0.0], move_cfg);

                if probe.hit_ground {
                    pos = probe.pos;

                    // Treat probe as a floor adhesion step: only vertical correction contributes to velocity.
                    // Horizontal push-out may happen due to corner cases; it should not become "player input speed".
                    applied[1] += probe.applied_motion[1];

                    hit_x |= probe.hit_x;
                    hit_y |= probe.hit_y;
                    hit_z |= probe.hit_z;

                    on_ground = true;
                }
            }
        }

        state.pos = pos;
        state.on_ground = on_ground;

        let inv_dt = 1.0 / dt_s;
        vel[0] = applied[0] * inv_dt;
        vel[1] = applied[1] * inv_dt;
        vel[2] = applied[2] * inv_dt;

        if hit_x {
            vel[0] = 0.0;
        }
        if hit_y {
            vel[1] = 0.0;
        }
        if hit_z {
            vel[2] = 0.0;
        }
        if state.on_ground && !jumped_this_tick {
            vel[1] = 0.0;
        }

        state.vel = vel;
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct HumanoidIntent {
    move_x: f32,
    move_z: f32,
    yaw_deg: f32,
    jump: bool,
    sprint: bool,
}

fn decode_intent(input: &CharacterControllerInput) -> HumanoidIntent {
    let raw = decode_humanoid_input_v1(&input.input).unwrap_or_default();
    HumanoidIntent {
        move_x: (raw.move_x as f32 / 127.0).clamp(-1.0, 1.0),
        move_z: (-(raw.move_z as f32) / 127.0).clamp(-1.0, 1.0),
        yaw_deg: input.view_yaw_deg,
        jump: (raw.buttons & button_bits::JUMP) != 0,
        sprint: (raw.buttons & button_bits::SPRINT) != 0,
    }
}

fn wish_dir_world(intent: &HumanoidIntent) -> (f32, f32) {
    let mut wish_x = intent.move_x;
    let mut wish_z = intent.move_z;
    let wish_len = (wish_x * wish_x + wish_z * wish_z).sqrt();
    if wish_len > 1.0 {
        let inv = 1.0 / wish_len;
        wish_x *= inv;
        wish_z *= inv;
    }

    let yaw_rad = normalize_yaw_deg(intent.yaw_deg).to_radians();
    let sin_y = yaw_rad.sin();
    let cos_y = yaw_rad.cos();

    // Camera-relative to world:
    // - move_x: strafe right
    // - move_z: forward (negative Z at yaw=0 in this convention)
    let wish_world_x = wish_x * cos_y - wish_z * sin_y;
    let wish_world_z = wish_x * sin_y + wish_z * cos_y;

    (wish_world_x, wish_world_z)
}

fn accelerate_horiz(vel: &mut [f32; 3], desired_vx: f32, desired_vz: f32, accel: f32, dt_s: f32) {
    let dvx = desired_vx - vel[0];
    let dvz = desired_vz - vel[2];
    let dv_len = (dvx * dvx + dvz * dvz).sqrt();

    let max_dv = accel * dt_s;
    if dv_len > max_dv && dv_len > f32::EPSILON {
        let s = max_dv / dv_len;
        vel[0] += dvx * s;
        vel[2] += dvz * s;
    } else {
        vel[0] += dvx;
        vel[2] += dvz;
    }
}

#[must_use]
pub fn humanoid_config() -> CharacterConfig {
    CharacterConfig {
        shape: CharacterShape::Aabb {
            half_extents: [0.3, 0.9, 0.3],
        },
        max_speed_ground: 4.5,
        max_speed_air: 4.5 * 0.65,
        accel_ground: 55.0,
        accel_air: 6.0,
        gravity: 16.0,
        jump_impulse: 7.0,
        step_height: 0.0, // MVP: no step-up yet (reserved for future controller logic)
        skin_width: 0.001,
    }
}

fn normalize_yaw_deg(yaw_deg: f32) -> f32 {
    (yaw_deg + 180.0).rem_euclid(360.0) - 180.0
}

fn shape_half_extents(shape: CharacterShape) -> Option<[f32; 3]> {
    static WARNED_UNSUPPORTED_HUMANOID_SHAPE: AtomicBool = AtomicBool::new(false);
    match shape {
        CharacterShape::Aabb { half_extents } => Some(half_extents),
        _ => {
            debug_assert!(false, "unsupported CharacterShape for humanoid controller");
            if !WARNED_UNSUPPORTED_HUMANOID_SHAPE.swap(true, Ordering::Relaxed) {
                freven_mod_api::emit_log(
                    LogLevel::Warn,
                    "unsupported CharacterShape for humanoid controller; skipping step",
                );
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freven_avatar_sdk_types::{KinematicMoveResult, SweepHit};

    struct FlatFloorPhysics;

    impl CharacterPhysics for FlatFloorPhysics {
        fn is_solid_world_collision(&mut self, _wx: i32, wy: i32, _wz: i32) -> bool {
            wy == 0
        }

        fn sweep_aabb(
            &mut self,
            _half_extents: [f32; 3],
            _from: [f32; 3],
            _to: [f32; 3],
        ) -> SweepHit {
            SweepHit::default()
        }

        fn move_aabb_terrain(
            &mut self,
            half_extents: [f32; 3],
            pos: [f32; 3],
            motion: [f32; 3],
            cfg: KinematicMoveConfig,
        ) -> KinematicMoveResult {
            // Minimal deterministic "floor at voxel y=0" physics:
            // - floor top plane is y=1.0 (voxel height)
            // - maintain a skin gap from the floor
            let floor_center_y = 1.0 + half_extents[1] + cfg.skin_width;

            let mut out = KinematicMoveResult {
                pos: [pos[0] + motion[0], pos[1] + motion[1], pos[2] + motion[2]],
                applied_motion: motion,
                ..Default::default()
            };

            if motion[1] < 0.0 && out.pos[1] < floor_center_y {
                out.pos[1] = floor_center_y;
                out.applied_motion[1] = out.pos[1] - pos[1];
                out.hit_y = true;
                out.hit_ground = true;
            } else if motion[1] > 0.0 && out.pos[1] < floor_center_y {
                // Not used by the test, but keep consistent: clamp upward into floor also "hits Y".
                out.pos[1] = floor_center_y;
                out.applied_motion[1] = out.pos[1] - pos[1];
                out.hit_y = true;
            }

            out
        }
    }

    #[test]
    fn grounded_stays_true_with_zero_gravity_via_floor_probe() {
        let mut controller = HumanoidController::new();

        // Disable gravity: without the floor-probe, out.hit_ground would never be set on idle ticks.
        controller.config.gravity = 0.0;

        let skin = controller.config.skin_width.abs().clamp(SKIN_MIN, SKIN_MAX);
        let half_extents = shape_half_extents(controller.config.shape)
            .expect("test humanoid controller shape must be supported");

        // Start exactly on floor contact with skin gap:
        // floor top at y=1.0, so center y is 1.0 + half_extents.y + skin.
        let start_y = 1.0 + half_extents[1] + skin;

        let mut state = CharacterState {
            pos: [0.5, start_y, 0.5],
            vel: [0.0, 0.0, 0.0],
            on_ground: true,
        };

        let input = CharacterControllerInput {
            input: std::sync::Arc::from([0_u8; 4].to_vec()),
            view_yaw_deg: 0.0,
            view_pitch_deg: 0.0,
            timeline: Default::default(),
        };
        let dt = Duration::from_secs_f32(1.0 / 30.0);

        let mut physics = FlatFloorPhysics;

        for _ in 0..120 {
            controller.step(&mut state, &input, &mut physics, dt);

            assert!(
                state.on_ground,
                "with gravity=0, grounded must remain true via floor-probe"
            );
            assert!(
                (state.pos[1] - start_y).abs() <= 1.0e-6,
                "idle on flat floor must not drift vertically: pos_y={} start_y={}",
                state.pos[1],
                start_y
            );
            assert!(
                state.vel[1].abs() <= 1.0e-6,
                "vertical velocity must remain ~0 when gravity=0 on flat floor: vy={}",
                state.vel[1]
            );
        }
    }
}
