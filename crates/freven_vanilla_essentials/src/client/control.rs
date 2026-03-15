use crate::humanoid_input::{HumanoidInputV1, button_bits, encode_humanoid_input_v1};
use freven_world_api::{
    ClientControlDeviceState, ClientControlOutput, ClientControlProvider,
    ClientControlProviderInit, ClientKeyCode,
};

const OWNER: &str = "freven.vanilla.essentials:movement";

pub const HUMANOID_CONTROL_KEY: &str = "freven.vanilla:humanoid_controls";

pub fn humanoid_control_provider_factory(
    _init: ClientControlProviderInit,
) -> Box<dyn ClientControlProvider> {
    Box::new(HumanoidControlProvider::new())
}

#[derive(Debug, Clone, Default)]
pub struct HumanoidControlProvider;

impl HumanoidControlProvider {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl ClientControlProvider for HumanoidControlProvider {
    fn sample(&mut self, device: &mut dyn ClientControlDeviceState) -> ClientControlOutput {
        let _ = device.bind_key(ClientKeyCode::KeyW, OWNER);
        let _ = device.bind_key(ClientKeyCode::KeyA, OWNER);
        let _ = device.bind_key(ClientKeyCode::KeyS, OWNER);
        let _ = device.bind_key(ClientKeyCode::KeyD, OWNER);
        let _ = device.bind_key(ClientKeyCode::Space, OWNER);
        let _ = device.bind_key(ClientKeyCode::Shift, OWNER);
        let _ = device.bind_key(ClientKeyCode::Ctrl, OWNER);

        let move_x = digital_axis_i8(
            device.key_down(ClientKeyCode::KeyA, OWNER),
            device.key_down(ClientKeyCode::KeyD, OWNER),
        );
        let move_z = digital_axis_i8(
            device.key_down(ClientKeyCode::KeyS, OWNER),
            device.key_down(ClientKeyCode::KeyW, OWNER),
        );

        let mut buttons = 0_u16;
        if device.key_down(ClientKeyCode::Space, OWNER) {
            buttons |= button_bits::JUMP;
        }
        if device.key_down(ClientKeyCode::Shift, OWNER) {
            buttons |= button_bits::SPRINT;
        }
        if device.key_down(ClientKeyCode::Ctrl, OWNER) {
            buttons |= button_bits::CROUCH;
        }

        let (yaw_deg, pitch_deg) = device.view_angles_deg();

        ClientControlOutput {
            input: std::sync::Arc::from(encode_humanoid_input_v1(HumanoidInputV1 {
                move_x,
                move_z,
                buttons,
            })),
            view_yaw_deg: yaw_deg,
            view_pitch_deg: pitch_deg,
        }
    }

    fn reset(&mut self) {
        // No internal state yet.
    }
}

fn digital_axis_i8(neg: bool, pos: bool) -> i8 {
    match (neg, pos) {
        (true, false) => -127,
        (false, true) => 127,
        _ => 0,
    }
}
