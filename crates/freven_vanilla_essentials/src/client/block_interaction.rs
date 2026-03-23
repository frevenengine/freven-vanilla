use std::sync::Arc;

use crate::action_payloads::{ActionTarget, encode_break_payload_v1, encode_place_payload_v1};
use crate::{STONE_KEY, break_action_kind_id, place_action_kind_id};
use freven_avatar_api::{ClientApi, ClientTickApi};
use freven_avatar_sdk_types::ClientMouseButton;
use freven_block_api::{ClientBlockFace, ClientPredictedEdit};
use freven_block_guest::{
    BlockQueryRequest, BlockQueryResponse, BlockServiceRequest, BlockServiceResponse,
};
use freven_block_sdk_types::BlockRuntimeId;
use freven_mod_api::LogLevel;
use freven_world_api::{
    ClientActionRequest, ClientActionSubmitError, WorldServiceRequest, WorldServiceResponse,
};

const OWNER: &str = "freven.vanilla.essentials:block_interaction";
const MAX_RAYCAST_DISTANCE_M: f32 = 5.0;
const BREAK_STATUS_FINISHED: u8 = 2;

pub fn start_client(api: &mut ClientApi<'_>) {
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
                predicted: vec![ClientPredictedEdit::clear_block(hit.block_pos)],
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
            let Some(place_block_id) =
                query_block_id_via_block_service(tick.client.services, STONE_KEY)
            else {
                log_local_skip(
                    tick,
                    action,
                    "place block id is not available in the client runtime",
                );
                return;
            };
            let Ok(place_wire_id) = u8::try_from(place_block_id.0) else {
                log_local_skip(
                    tick,
                    action,
                    "place block id does not fit the current vanilla action payload format",
                );
                return;
            };

            let payload = encode_place_payload_v1(target, place_wire_id);

            let req = ClientActionRequest {
                action_kind_id: place_action_kind_id(),
                payload: Arc::from(payload),
                at_input_seq,
                predicted: vec![ClientPredictedEdit {
                    pos: place_pos,
                    predicted_block_id: place_block_id,
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

/// Resolve a standard block runtime id through the block-owned query contract.
///
/// `BlockQueryRequest::BlockIdByKey` is owned by `freven_block_guest`.
/// `WorldServiceRequest::Block(...)` is only the generic runtime-service carrier
/// used by the client runtime path.
fn query_block_id_via_block_service(
    services: &mut dyn freven_world_api::Services,
    key: &str,
) -> Option<BlockRuntimeId> {
    match services.world_service(&WorldServiceRequest::Block(BlockServiceRequest::Query(
        BlockQueryRequest::BlockIdByKey {
            key: key.to_string(),
        },
    ))) {
        WorldServiceResponse::Block(BlockServiceResponse::Query(
            BlockQueryResponse::BlockIdByKey(value),
        )) => value,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freven_avatar_sdk_types::{
        ClientInputProvider, ClientKeyCode, ClientPlayerProvider, ClientPlayerView,
    };
    use freven_block_api::{ClientCameraHitProvider, ClientCameraRay, ClientCursorHit};
    use freven_block_sdk_types::BlockRuntimeId;
    use freven_world_api::{
        ActionKindId, ClientActionResultEvent, ClientInteractionProvider, ComponentId,
    };

    #[derive(Default)]
    struct NoopServices;

    impl Services for NoopServices {
        fn world_service(&mut self, request: &WorldServiceRequest) -> WorldServiceResponse {
            match request {
                WorldServiceRequest::Block(BlockServiceRequest::Query(
                    BlockQueryRequest::BlockIdByKey { key },
                )) if key == STONE_KEY => WorldServiceResponse::Block(BlockServiceResponse::Query(
                    BlockQueryResponse::BlockIdByKey(Some(BlockRuntimeId(3))),
                )),
                _ => WorldServiceResponse::Unsupported,
            }
        }
    }

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

        fn predicted_block_id_at(&self, _pos: (i32, i32, i32)) -> Option<BlockRuntimeId> {
            panic!("block interaction submit path must not query prediction-aware block ids");
        }

        fn authoritative_block_id_at(&self, _pos: (i32, i32, i32)) -> Option<BlockRuntimeId> {
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

        {
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut camera,
                &mut interaction,
                &mut players,
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
            vec![ClientPredictedEdit::clear_block((4, 5, 6))]
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

        {
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut camera,
                &mut interaction,
                &mut players,
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
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
    }
}
