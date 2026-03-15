use std::sync::Arc;

use crate::action_payloads::{ActionTarget, encode_break_payload_v1, encode_place_payload_v1};
use crate::storage_ids;
use crate::{break_action_kind_id, place_action_kind_id};
use freven_world_api::{
    ClientActionRequest, ClientActionSubmitError, ClientBlockFace, ClientMouseButton,
    ClientPredictedEdit, ClientTickApi, LogLevel,
};

const OWNER: &str = "freven.vanilla.essentials:block_interaction";
const MAX_RAYCAST_DISTANCE_M: f32 = 5.0;
const BREAK_STATUS_FINISHED: u8 = 2;
const PLACE_BLOCK_ID: u8 = storage_ids::STONE_U8;

pub fn start_client(api: &mut freven_world_api::ClientApi<'_>) {
    let _ = api.input.bind_mouse_button(ClientMouseButton::Left, OWNER);
    let _ = api.input.bind_mouse_button(ClientMouseButton::Right, OWNER);
}

pub fn tick_client(tick: &mut ClientTickApi<'_>) {
    // Consume one click per tick (owner-guarded).
    let action = {
        let api = &mut tick.client;
        if api
            .input
            .consume_mouse_button_press(ClientMouseButton::Left, OWNER)
        {
            Some(ClientMouseButton::Left)
        } else if api
            .input
            .consume_mouse_button_press(ClientMouseButton::Right, OWNER)
        {
            Some(ClientMouseButton::Right)
        } else {
            None
        }
    };

    let Some(action) = action else {
        return;
    };

    // We only allow submitting actions when the client has an active stream.
    if tick.client.interaction.active_stream().is_none() {
        log_local_skip(tick, action, "no active world stream");
        return;
    }

    let Some(hit) = tick
        .client
        .camera
        .authoritative_cursor_hit(MAX_RAYCAST_DISTANCE_M)
    else {
        log_local_skip(tick, action, "no authoritative block target under cursor");
        return;
    };

    let Some(target_face) = client_face_to_wire(hit.face) else {
        log_local_skip(
            tick,
            action,
            "unsupported block face from authoritative hit",
        );
        return;
    };
    let at_input_seq = tick.client.interaction.next_input_seq();
    let target = ActionTarget {
        pos: hit.block_pos,
        face: target_face,
    };

    let submit_failure = match action {
        ClientMouseButton::Left => {
            let payload = encode_break_payload_v1(BREAK_STATUS_FINISHED, target);

            let req = ClientActionRequest {
                action_kind_id: break_action_kind_id(),
                payload: Arc::from(payload),
                at_input_seq,
                predicted: vec![ClientPredictedEdit {
                    pos: hit.block_pos,
                    predicted_block_id: storage_ids::AIR_U8,
                }],
            };

            // Engine assigns action_seq and owns retransmit/prediction.
            tick.client
                .interaction
                .submit_action(req)
                .err()
                .map(|err| ("break", err))
        }

        ClientMouseButton::Right => {
            // Place: compute adjacent placement cell.
            let Some(place_pos) = add_face_offset(hit.block_pos, hit.face) else {
                log_local_skip(
                    tick,
                    action,
                    "placement target overflowed world coordinates",
                );
                return;
            };

            let payload = encode_place_payload_v1(target, PLACE_BLOCK_ID);

            let req = ClientActionRequest {
                action_kind_id: place_action_kind_id(),
                payload: Arc::from(payload),
                at_input_seq,
                predicted: vec![ClientPredictedEdit {
                    pos: place_pos,
                    predicted_block_id: PLACE_BLOCK_ID,
                }],
            };

            tick.client
                .interaction
                .submit_action(req)
                .err()
                .map(|err| ("place", err))
        }

        ClientMouseButton::Middle => None,
        _ => None,
    };

    if let Some((action, err)) = submit_failure {
        log_submit_failure(tick, action, err);
    }
}

fn log_local_skip(tick: &mut ClientTickApi<'_>, action: ClientMouseButton, reason: &str) {
    tick.log(
        LogLevel::Debug,
        format!(
            "{} interaction not submitted: {reason}",
            action_name(action)
        ),
    );
}

fn log_submit_failure(tick: &mut ClientTickApi<'_>, action: &str, err: ClientActionSubmitError) {
    tick.log(
        LogLevel::Warn,
        format!("failed to submit {action} action: {err}"),
    );
}

fn action_name(action: ClientMouseButton) -> &'static str {
    match action {
        ClientMouseButton::Left => "break",
        ClientMouseButton::Right => "place",
        ClientMouseButton::Middle => "middle",
        _ => "other",
    }
}

fn client_face_to_wire(face: ClientBlockFace) -> Option<u8> {
    match face {
        ClientBlockFace::NegX => Some(0),
        ClientBlockFace::PosX => Some(1),
        ClientBlockFace::NegY => Some(2),
        ClientBlockFace::PosY => Some(3),
        ClientBlockFace::NegZ => Some(4),
        ClientBlockFace::PosZ => Some(5),
        _ => None,
    }
}

fn add_face_offset(pos: (i32, i32, i32), face: ClientBlockFace) -> Option<(i32, i32, i32)> {
    let (x, y, z) = pos;
    match face {
        ClientBlockFace::PosX => x.checked_add(1).map(|nx| (nx, y, z)),
        ClientBlockFace::NegX => x.checked_sub(1).map(|nx| (nx, y, z)),
        ClientBlockFace::PosY => y.checked_add(1).map(|ny| (x, ny, z)),
        ClientBlockFace::NegY => y.checked_sub(1).map(|ny| (x, ny, z)),
        ClientBlockFace::PosZ => z.checked_add(1).map(|nz| (x, y, nz)),
        ClientBlockFace::NegZ => z.checked_sub(1).map(|nz| (x, y, nz)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freven_world_api::{
        ActionKindId, ClientActionResultEvent, ClientCameraHitProvider, ClientCameraRay,
        ClientCursorHit, ClientInputProvider, ClientInteractionProvider, ClientKeyCode,
        ClientNameplateDrawCmd, ClientNameplateProvider, ClientPlayerProvider, ClientPlayerView,
        ComponentId, Services,
    };

    #[derive(Default)]
    struct NoopServices;

    impl Services for NoopServices {}

    struct TestInput {
        left: bool,
        right: bool,
    }

    impl ClientInputProvider for TestInput {
        fn mouse_button_down(&self, _button: ClientMouseButton) -> bool {
            false
        }

        fn mouse_button_just_pressed(&self, _button: ClientMouseButton) -> bool {
            false
        }

        fn key_down(&self, _key: ClientKeyCode) -> bool {
            false
        }

        fn key_just_pressed(&self, _key: ClientKeyCode) -> bool {
            false
        }

        fn bind_mouse_button(&mut self, _button: ClientMouseButton, _owner: &str) -> bool {
            true
        }

        fn bind_key(&mut self, _key: ClientKeyCode, _owner: &str) -> bool {
            true
        }

        fn consume_mouse_button_press(&mut self, button: ClientMouseButton, _owner: &str) -> bool {
            match button {
                ClientMouseButton::Left => std::mem::take(&mut self.left),
                ClientMouseButton::Right => std::mem::take(&mut self.right),
                _ => false,
            }
        }

        fn consume_key_press(&mut self, _key: ClientKeyCode, _owner: &str) -> bool {
            false
        }
    }

    struct AuthoritativeOnlyCamera {
        hit: ClientCursorHit,
    }

    impl ClientCameraHitProvider for AuthoritativeOnlyCamera {
        fn camera_ray(&self) -> Option<ClientCameraRay> {
            None
        }

        fn authoritative_cursor_hit(&self, _max_distance_m: f32) -> Option<ClientCursorHit> {
            Some(self.hit)
        }

        fn predicted_cursor_hit(&self, _max_distance_m: f32) -> Option<ClientCursorHit> {
            panic!("block interaction submit path must not use prediction-aware cursor hits");
        }

        fn predicted_block_id_at(&self, _pos: (i32, i32, i32)) -> Option<u8> {
            panic!("block interaction submit path must not query prediction-aware block ids");
        }

        fn authoritative_block_id_at(&self, _pos: (i32, i32, i32)) -> Option<u8> {
            panic!("block interaction submit path must not query authoritative block ids");
        }
    }

    #[derive(Default)]
    struct RecordingInteraction {
        requests: Vec<ClientActionRequest>,
    }

    impl ClientInteractionProvider for RecordingInteraction {
        fn active_stream(&self) -> Option<(u32, u32)> {
            Some((1, 1))
        }

        fn next_input_seq(&self) -> u32 {
            42
        }

        fn submit_action(
            &mut self,
            req: ClientActionRequest,
        ) -> Result<u32, freven_world_api::ClientActionSubmitError> {
            self.requests.push(req);
            Ok(self.requests.len() as u32)
        }

        fn poll_action_result(&mut self) -> Option<ClientActionResultEvent> {
            None
        }
    }

    #[derive(Default)]
    struct NoopPlayers;

    impl ClientPlayerProvider for NoopPlayers {
        fn list_players(&self, _out: &mut Vec<ClientPlayerView>) {}

        fn display_name_for(&self, _player_id: u64) -> Option<String> {
            None
        }

        fn component_bytes_for(
            &self,
            _player_id: u64,
            _component_id: ComponentId,
        ) -> Option<&[u8]> {
            None
        }

        fn world_to_screen(&self, _world_pos_m: (f32, f32, f32)) -> Option<(f32, f32)> {
            None
        }
    }

    #[derive(Default)]
    struct NoopNameplates;

    impl ClientNameplateProvider for NoopNameplates {
        fn clear_nameplates(&mut self) {}

        fn push_nameplate(&mut self, _cmd: ClientNameplateDrawCmd) {}
    }

    fn ensure_action_kinds() {
        let _ = crate::VANILLA_ACTION_KINDS.get_or_init(|| crate::VanillaActionKinds {
            break_kind: ActionKindId(1),
            place_kind: ActionKindId(2),
        });
    }

    #[test]
    fn left_click_submits_break_without_block_query_gates() {
        ensure_action_kinds();

        let mut services = NoopServices;
        let mut input = TestInput {
            left: true,
            right: false,
        };
        let mut camera = AuthoritativeOnlyCamera {
            hit: ClientCursorHit {
                block_pos: (4, 5, 6),
                face: ClientBlockFace::PosX,
                distance_m: 1.5,
            },
        };
        let mut interaction = RecordingInteraction::default();
        let mut players = NoopPlayers;
        let mut nameplates = NoopNameplates;

        {
            let client = freven_world_api::ClientApi::new(
                &mut services,
                &mut input,
                &mut camera,
                &mut interaction,
                &mut players,
                &mut nameplates,
            );
            let mut tick = ClientTickApi::new(7, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert_eq!(interaction.requests.len(), 1);
        let req = &interaction.requests[0];
        assert_eq!(req.action_kind_id, break_action_kind_id());
        assert_eq!(req.at_input_seq, 42);
        assert_eq!(
            req.predicted,
            vec![ClientPredictedEdit {
                pos: (4, 5, 6),
                predicted_block_id: storage_ids::AIR_U8,
            }]
        );
    }

    #[test]
    fn right_click_submits_place_without_block_query_gates() {
        ensure_action_kinds();

        let mut services = NoopServices;
        let mut input = TestInput {
            left: false,
            right: true,
        };
        let mut camera = AuthoritativeOnlyCamera {
            hit: ClientCursorHit {
                block_pos: (10, 20, 30),
                face: ClientBlockFace::PosY,
                distance_m: 2.0,
            },
        };
        let mut interaction = RecordingInteraction::default();
        let mut players = NoopPlayers;
        let mut nameplates = NoopNameplates;

        {
            let client = freven_world_api::ClientApi::new(
                &mut services,
                &mut input,
                &mut camera,
                &mut interaction,
                &mut players,
                &mut nameplates,
            );
            let mut tick = ClientTickApi::new(9, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert_eq!(interaction.requests.len(), 1);
        let req = &interaction.requests[0];
        assert_eq!(req.action_kind_id, place_action_kind_id());
        assert_eq!(req.at_input_seq, 42);
        assert_eq!(
            req.predicted,
            vec![ClientPredictedEdit {
                pos: (10, 21, 30),
                predicted_block_id: PLACE_BLOCK_ID,
            }]
        );
    }
}
