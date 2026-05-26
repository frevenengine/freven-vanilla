use std::sync::Arc;

use crate::action_payloads::{ActionTarget, encode_break_payload_v1, encode_place_payload_v1};
use crate::{STONE_KEY, break_action_kind_id, place_action_kind_id};
use freven_avatar_api::{ClientApi, ClientTickApi};
use freven_avatar_sdk_types::ClientMouseButton;
use freven_block_api::{
    ClientBlockFace, ClientCameraHitProvider, ClientCursorHit, ClientPredictedEdit,
};
use freven_block_guest::{
    BlockQueryRequest, BlockQueryResponse, BlockServiceRequest, BlockServiceResponse,
};
use freven_block_sdk_types::BlockRuntimeId;
use freven_mod_api::LogLevel;
use freven_world_api::{
    ClientActionRequest, ClientActionSubmitError, Services, WorldServiceRequest,
    WorldServiceResponse,
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

    let at_input_seq = tick.client.interaction.next_input_seq();

    let submit_failure = match action {
        ClientMouseButton::Left => {
            let Some(target) = select_presented_break_target(tick.client.camera) else {
                log_local_skip(tick, action, missing_target_reason(action));
                return;
            };

            let payload = encode_break_payload_v1(BREAK_STATUS_FINISHED, target.action_target);

            let req = ClientActionRequest {
                action_kind_id: break_action_kind_id(),
                payload: Arc::from(payload),
                at_input_seq,
                predicted: vec![ClientPredictedEdit::clear_block(target.hit.block_pos)],
            };

            // Engine assigns action_seq and owns retransmit/prediction.
            tick.client
                .interaction
                .submit_action(req)
                .err()
                .map(|err| ("break", err))
        }

        ClientMouseButton::Right => {
            let Some(target) = select_presented_place_target(tick.client.camera) else {
                log_local_skip(tick, action, missing_target_reason(action));
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

            let payload = encode_place_payload_v1(target.action_target, place_wire_id);

            let req = ClientActionRequest {
                action_kind_id: place_action_kind_id(),
                payload: Arc::from(payload),
                at_input_seq,
                predicted: vec![ClientPredictedEdit {
                    pos: target.place_pos,
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

fn missing_target_reason(action: ClientMouseButton) -> &'static str {
    match action {
        ClientMouseButton::Left => "no presented solid block target under cursor",
        ClientMouseButton::Right => "no presented place target under cursor",
        _ => "no block target under cursor",
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BreakInteractionTarget {
    hit: ClientCursorHit,
    action_target: ActionTarget,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PlaceInteractionTarget {
    action_target: ActionTarget,
    place_pos: (i32, i32, i32),
}

fn select_presented_break_target(
    camera: &dyn ClientCameraHitProvider,
) -> Option<BreakInteractionTarget> {
    let hit = camera.predicted_cursor_hit(MAX_RAYCAST_DISTANCE_M)?;
    let hit_block = camera.predicted_block_id_at(hit.block_pos)?;
    if hit_block.0 == 0 {
        return None;
    }

    Some(BreakInteractionTarget {
        hit,
        action_target: action_target_from_hit(hit)?,
    })
}

fn select_presented_place_target(
    camera: &dyn ClientCameraHitProvider,
) -> Option<PlaceInteractionTarget> {
    let hit = camera.predicted_cursor_hit(MAX_RAYCAST_DISTANCE_M)?;
    let hit_block = camera.predicted_block_id_at(hit.block_pos)?;
    if hit_block.0 == 0 {
        return None;
    }

    let place_pos = add_face_offset(hit.block_pos, hit.face)?;
    let place_cur = camera.predicted_block_id_at(place_pos)?;
    if place_cur.0 != 0 {
        return None;
    }

    Some(PlaceInteractionTarget {
        action_target: action_target_from_hit(hit)?,
        place_pos,
    })
}

fn action_target_from_hit(hit: ClientCursorHit) -> Option<ActionTarget> {
    Some(ActionTarget {
        pos: hit.block_pos,
        face: client_face_to_wire(hit.face)?,
    })
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
        submit_failure_log_level(&err),
        format!("failed to submit {action} action: {err}"),
    );
}

fn submit_failure_log_level(err: &ClientActionSubmitError) -> LogLevel {
    match err {
        ClientActionSubmitError::LocalPredictionNoop
        | ClientActionSubmitError::LocalPredictionConflict
        | ClientActionSubmitError::LocalPredictionBacklog { .. } => LogLevel::Debug,
        _ => LogLevel::Warn,
    }
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
    services: &mut dyn Services,
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

    #[test]
    fn local_prediction_admission_submit_failures_log_at_debug() {
        assert!(matches!(
            submit_failure_log_level(&ClientActionSubmitError::LocalPredictionNoop),
            LogLevel::Debug
        ));
        assert!(matches!(
            submit_failure_log_level(&ClientActionSubmitError::LocalPredictionConflict),
            LogLevel::Debug
        ));
        assert!(matches!(
            submit_failure_log_level(&ClientActionSubmitError::LocalPredictionBacklog {
                pending: 8,
                limit: 8,
            }),
            LogLevel::Debug
        ));
    }

    #[test]
    fn non_local_prediction_submit_failures_stay_warn() {
        assert!(matches!(
            submit_failure_log_level(&ClientActionSubmitError::NoActiveStream),
            LogLevel::Warn
        ));
        assert!(matches!(
            submit_failure_log_level(&ClientActionSubmitError::TooManyPending),
            LogLevel::Warn
        ));
    }
    use crate::action_payloads::{decode_break_payload_v1, decode_place_payload_v1};
    use freven_avatar_sdk_types::{
        ClientInputProvider, ClientKeyCode, ClientPlayerProvider, ClientPlayerView,
    };
    use freven_block_api::{
        BlockAuthority, BlockMutationResult, BlockWorldView, ClientCameraHitProvider,
        ClientCameraRay, ClientCursorHit,
    };
    use freven_block_guest::BlockMutation;
    use freven_block_sdk_types::BlockRuntimeId;
    use freven_world_api::{
        ActionCmdView, ActionContext, ActionHandler, ActionKindId, ActionOutcome,
        CharacterPhysicsQuery, ClientActionResultEvent, ClientInteractionProvider, ComponentId,
        Services,
    };
    use std::collections::HashMap;

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

    struct PresentedCamera {
        predicted_hit: Option<ClientCursorHit>,
        blocks: HashMap<(i32, i32, i32), BlockRuntimeId>,
    }

    impl PresentedCamera {
        fn new(predicted_hit: ClientCursorHit) -> Self {
            Self {
                predicted_hit: Some(predicted_hit),
                blocks: HashMap::new(),
            }
        }

        fn with_block(mut self, pos: (i32, i32, i32), block_id: u32) -> Self {
            self.blocks.insert(pos, BlockRuntimeId(block_id));
            self
        }
    }

    impl ClientCameraHitProvider for PresentedCamera {
        fn camera_ray(&self) -> Option<ClientCameraRay> {
            None
        }

        fn authoritative_cursor_hit(&self, _max_distance_m: f32) -> Option<ClientCursorHit> {
            panic!("block interaction submit path must use prediction-aware cursor hits");
        }

        fn predicted_cursor_hit(&self, _max_distance_m: f32) -> Option<ClientCursorHit> {
            self.predicted_hit
        }

        fn predicted_block_id_at(&self, pos: (i32, i32, i32)) -> Option<BlockRuntimeId> {
            Some(*self.blocks.get(&pos).unwrap_or(&BlockRuntimeId(0)))
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

    #[derive(Default)]
    struct TestAuthority {
        blocks: HashMap<(i32, i32, i32), BlockRuntimeId>,
    }

    impl TestAuthority {
        fn with_block(mut self, pos: (i32, i32, i32), block_id: u32) -> Self {
            self.blocks.insert(pos, BlockRuntimeId(block_id));
            self
        }
    }

    impl BlockWorldView for TestAuthority {
        fn block(&self, wx: i32, wy: i32, wz: i32) -> Option<BlockRuntimeId> {
            Some(*self.blocks.get(&(wx, wy, wz)).unwrap_or(&BlockRuntimeId(0)))
        }

        fn is_solid(&self, block_id: BlockRuntimeId) -> bool {
            block_id.0 != 0
        }
    }

    impl BlockAuthority for TestAuthority {
        fn try_apply(&mut self, mutation: &BlockMutation) -> BlockMutationResult {
            match mutation {
                BlockMutation::SetBlock {
                    pos,
                    block_id,
                    expected_old,
                } => {
                    let current = self.block(pos.0, pos.1, pos.2).unwrap_or(BlockRuntimeId(0));
                    if expected_old.is_some_and(|expected| expected != current) {
                        return BlockMutationResult::Mismatch { current };
                    }
                    self.blocks.insert(*pos, *block_id);
                    BlockMutationResult::Applied {
                        old: current,
                        new: *block_id,
                    }
                }
                _ => BlockMutationResult::Rejected {
                    message: "unsupported mutation in test".to_string(),
                },
            }
        }
    }

    struct TestPhysics;

    impl CharacterPhysicsQuery for TestPhysics {
        fn player_position(&self, _player_id: u64) -> Option<[f32; 3]> {
            Some([9.5, 18.0, 30.5])
        }
    }

    fn ensure_action_kinds() {
        let _ = crate::VANILLA_ACTION_KINDS.get_or_init(|| crate::VanillaActionKinds {
            break_kind: ActionKindId(1),
            place_kind: ActionKindId(2),
        });
    }

    #[test]
    fn left_click_submits_break_from_presented_cursor() {
        ensure_action_kinds();

        let mut services = NoopServices;
        let mut input = TestInput {
            left: true,
            right: false,
        };
        let mut camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (4, 5, 6),
            face: ClientBlockFace::PosX,
            distance_m: 1.5,
        })
        .with_block((4, 5, 6), 1);
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
        let payload = decode_break_payload_v1(&req.payload).expect("decode break");
        assert_eq!(payload.target.pos, (4, 5, 6));
        assert_eq!(
            req.predicted,
            vec![ClientPredictedEdit::clear_block((4, 5, 6))]
        );
    }

    #[test]
    fn double_click_break_uses_next_presented_solid() {
        ensure_action_kinds();

        let mut services = NoopServices;
        let mut input = TestInput {
            left: true,
            right: false,
        };
        let mut camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (5, 5, 6),
            face: ClientBlockFace::NegX,
            distance_m: 2.5,
        })
        .with_block((4, 5, 6), 0)
        .with_block((5, 5, 6), 1);
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
            let mut tick = ClientTickApi::new(8, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert_eq!(interaction.requests.len(), 1);
        let req = &interaction.requests[0];
        let payload = decode_break_payload_v1(&req.payload).expect("decode break");
        assert_eq!(payload.target.pos, (5, 5, 6));
        assert_eq!(
            req.predicted,
            vec![ClientPredictedEdit::clear_block((5, 5, 6))]
        );
    }

    #[test]
    fn right_click_places_into_presented_empty_cell_before_next_solid() {
        ensure_action_kinds();

        let mut services = NoopServices;
        let mut input = TestInput {
            left: false,
            right: true,
        };
        let mut camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (11, 20, 30),
            face: ClientBlockFace::NegX,
            distance_m: 2.0,
        })
        .with_block((10, 20, 30), 0)
        .with_block((11, 20, 30), 1);
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
        let payload = decode_place_payload_v1(&req.payload).expect("decode place");
        assert_eq!(payload.target.pos, (11, 20, 30));
        assert_eq!(payload.target.face, 0);
        assert_eq!(
            req.predicted,
            vec![ClientPredictedEdit {
                pos: (10, 20, 30),
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
    }

    #[test]
    fn place_does_not_submit_against_presented_air_hit() {
        ensure_action_kinds();

        let mut services = NoopServices;
        let mut input = TestInput {
            left: false,
            right: true,
        };
        let mut camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (10, 20, 30),
            face: ClientBlockFace::PosY,
            distance_m: 2.0,
        })
        .with_block((10, 20, 30), 0)
        .with_block((10, 21, 30), 0);
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
            let mut tick = ClientTickApi::new(10, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert!(interaction.requests.is_empty());
    }

    #[test]
    fn server_authoritative_validation_rejects_predicted_only_targets() {
        let physics = TestPhysics;
        let break_payload = encode_break_payload_v1(
            BREAK_STATUS_FINISHED,
            ActionTarget {
                pos: (10, 20, 30),
                face: 0,
            },
        );
        let break_cmd = ActionCmdView {
            action_kind: ActionKindId(1),
            level_id: 1,
            stream_epoch: 1,
            seq: 1,
            at_input_seq: 42,
            payload: &break_payload,
        };
        let mut services = NoopServices;
        let mut break_authority = TestAuthority::default().with_block((10, 20, 30), 0);
        let mut break_ctx = ActionContext::new(
            None,
            Some(&mut break_authority),
            Some(&physics),
            Some(&mut services),
            7,
            42,
        );

        let mut break_handler = crate::actions::r#break::BreakActionHandler;
        assert_eq!(
            break_handler.handle(&mut break_ctx, &break_cmd),
            ActionOutcome::Rejected
        );

        let place_payload = encode_place_payload_v1(
            ActionTarget {
                pos: (11, 20, 30),
                face: 0,
            },
            3,
        );
        let place_cmd = ActionCmdView {
            action_kind: ActionKindId(2),
            level_id: 1,
            stream_epoch: 1,
            seq: 2,
            at_input_seq: 43,
            payload: &place_payload,
        };
        let mut place_authority = TestAuthority::default()
            .with_block((10, 20, 30), 0)
            .with_block((11, 20, 30), 0);
        let mut place_ctx = ActionContext::new(
            None,
            Some(&mut place_authority),
            Some(&physics),
            Some(&mut services),
            7,
            43,
        );

        let mut place_handler = crate::actions::place::PlaceActionHandler;
        assert_eq!(
            place_handler.handle(&mut place_ctx, &place_cmd),
            ActionOutcome::Rejected
        );
    }

    #[test]
    fn rapid_alternating_break_place_keeps_real_presented_changes() {
        ensure_action_kinds();

        let mut services = NoopServices;
        let mut input = TestInput {
            left: false,
            right: true,
        };
        let mut place_camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (11, 20, 30),
            face: ClientBlockFace::NegX,
            distance_m: 2.0,
        })
        .with_block((10, 20, 30), 0)
        .with_block((11, 20, 30), 1);
        let mut interaction = RecordingInteraction::default();
        let mut players = NoopPlayers;

        {
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut place_camera,
                &mut interaction,
                &mut players,
            );
            let mut tick = ClientTickApi::new(11, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        let mut input = TestInput {
            left: true,
            right: false,
        };
        let mut break_camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (10, 20, 30),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((10, 20, 30), 3)
        .with_block((11, 20, 30), 1);

        {
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut break_camera,
                &mut interaction,
                &mut players,
            );
            let mut tick = ClientTickApi::new(12, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert_eq!(interaction.requests.len(), 2);
        assert_eq!(
            interaction.requests[0].predicted,
            vec![ClientPredictedEdit {
                pos: (10, 20, 30),
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
        assert_eq!(
            interaction.requests[1].predicted,
            vec![ClientPredictedEdit::clear_block((10, 20, 30))]
        );
    }
}
