use std::cell::RefCell;
use std::sync::Arc;

use crate::action_payloads::{try_encode_break_payload_v2, try_encode_place_payload_v2};
use crate::actions::targeting::{MAX_ACTION_REACH_M, humanoid_interaction_origin_m};
use crate::{STONE_KEY, break_action_kind_id, place_action_kind_id};
use freven_avatar_api::{ClientApi, ClientTickApi};
use freven_avatar_sdk_types::{ClientMouseButton, ClientPlayerProvider, ClientPlayerView};
use freven_block_api::{
    ClientBlockFace, ClientCameraHitProvider, ClientCameraRay, ClientCursorHit,
    ClientPredictedEdit, TerrainInteractionCellV2, TerrainInteractionHitV2,
    TerrainInteractionIdentityV2, TerrainInteractionIntentV2, TerrainInteractionKindV2,
    TerrainInteractionRayV2, TerrainInteractionRejectReasonV2, TerrainInteractionRulesV2,
    TerrainInteractionValidationPolicyV2, TerrainInteractionValidationV2,
    TerrainInteractionWorldViewV2, TerrainPlaceIntentV2, TerrainPredictionTransactionIdV2,
    validate_terrain_interaction_v2,
};
use freven_block_guest::{
    BlockQueryRequest, BlockQueryResponse, BlockServiceRequest, BlockServiceResponse,
};
use freven_block_sdk_types::BlockRuntimeId;
use freven_mod_api::{LogLevel, emit_log};
use freven_world_api::{
    ClientActionRequest, ClientActionSubmitError, Services, WorldServiceRequest,
    WorldServiceResponse,
};

const OWNER: &str = "freven.vanilla.essentials:block_interaction";
const MAX_RAYCAST_DISTANCE_M: f32 = 5.0;
const MAX_MOUSE_PRESSES_PER_TICK: usize = 8;
thread_local! {
    static LOCAL_TERRAIN_PREDICTIONS: RefCell<LocalTerrainPredictionState> =
        const { RefCell::new(LocalTerrainPredictionState {
        pending: Vec::new(),
    }) };
}

pub fn start_client(api: &mut ClientApi<'_>) {
    let _ = api.input.bind_mouse_button(ClientMouseButton::Left, OWNER);
    let _ = api.input.bind_mouse_button(ClientMouseButton::Right, OWNER);
    clear_local_terrain_predictions();
}

pub fn tick_client(tick: &mut ClientTickApi<'_>) {
    prune_local_terrain_predictions(tick.client.camera);

    // Drain a bounded ordered click batch. This preserves repeated RMB clicks
    // and LMB/RMB ordering captured by the engine input queue while avoiding
    // unbounded action spam in one client tick.
    let presses = tick
        .client
        .input
        .drain_mouse_button_presses(OWNER, MAX_MOUSE_PRESSES_PER_TICK);

    if !presses.is_empty() {
        tracing::debug!(
            target: "freven_vanilla_essentials::client::block_interaction",
            owner = OWNER,
            press_count = presses.len(),
            presses = ?presses,
            "drained block interaction mouse presses",
        );
    }

    for press in presses {
        tracing::debug!(
            target: "freven_vanilla_essentials::client::block_interaction",
            action = ?press.button,
            "handling block interaction mouse press",
        );
        handle_mouse_press_action(tick, press.button);
    }
}

fn handle_mouse_press_action(tick: &mut ClientTickApi<'_>, action: ClientMouseButton) {
    // We only allow submitting actions when the client has an active stream.
    let Some((level_id, stream_epoch)) = tick.client.interaction.active_stream() else {
        log_local_skip(tick, action, "no active world stream");
        return;
    };

    let at_input_seq = tick.client.interaction.next_input_seq();
    let client_view_tick = tick.tick;

    match action {
        ClientMouseButton::Left => {
            let Some(target) = select_presented_break_target(tick.client.camera) else {
                log_local_skip(tick, action, missing_target_reason(action));
                return;
            };

            let intent = build_break_intent(
                level_id,
                stream_epoch,
                at_input_seq,
                client_view_tick,
                target,
            );
            let admission = match validate_local_prediction_admission(
                tick.client.camera,
                tick.client.players,
                None,
                &intent,
            ) {
                Ok(admission) => admission,
                Err(reason) => {
                    log_local_validation_reject(tick, "break", &intent, reason);
                    return;
                }
            };

            let payload = match try_encode_break_payload_v2(&intent) {
                Ok(payload) => payload,
                Err(err) => {
                    log_encode_failure(tick, action, &err);
                    return;
                }
            };

            let predicted = vec![ClientPredictedEdit::clear_block(target.hit.block_pos)];
            let req = ClientActionRequest {
                action_kind_id: break_action_kind_id(),
                payload: Arc::from(payload),
                at_input_seq,
                predicted: predicted.clone(),
            };

            // Engine assigns action_seq and owns retransmit/prediction.
            match tick.client.interaction.submit_action(req) {
                Ok(action_seq) => {
                    remember_local_terrain_predictions(
                        action_seq,
                        intent.identity.prediction_tx,
                        &predicted,
                    );
                    log_local_prediction_accepted(tick, "break", action_seq, &intent, admission);
                }
                Err(err) => log_submit_failure(tick, "break", err),
            }
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
            let intent = build_place_intent(
                level_id,
                stream_epoch,
                at_input_seq,
                client_view_tick,
                target,
                place_block_id,
            );
            let admission = match validate_local_prediction_admission(
                tick.client.camera,
                tick.client.players,
                Some(place_block_id),
                &intent,
            ) {
                Ok(admission) => admission,
                Err(reason) => {
                    log_local_validation_reject(tick, "place", &intent, reason);
                    return;
                }
            };

            let payload = match try_encode_place_payload_v2(&intent) {
                Ok(payload) => payload,
                Err(err) => {
                    log_encode_failure(tick, action, &err);
                    return;
                }
            };

            let predicted = vec![ClientPredictedEdit {
                pos: target.place_pos,
                predicted_block_id: place_block_id,
            }];
            let req = ClientActionRequest {
                action_kind_id: place_action_kind_id(),
                payload: Arc::from(payload),
                at_input_seq,
                predicted: predicted.clone(),
            };

            match tick.client.interaction.submit_action(req) {
                Ok(action_seq) => {
                    remember_local_terrain_predictions(
                        action_seq,
                        intent.identity.prediction_tx,
                        &predicted,
                    );
                    log_local_prediction_accepted(tick, "place", action_seq, &intent, admission);
                }
                Err(err) => log_submit_failure(tick, "place", err),
            }
        }

        ClientMouseButton::Middle => {}
        _ => {}
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
    camera_ray: ClientCameraRay,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PlaceInteractionTarget {
    hit: ClientCursorHit,
    camera_ray: ClientCameraRay,
    place_pos: (i32, i32, i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingTerrainPrediction {
    action_seq: u32,
    prediction_tx: TerrainPredictionTransactionIdV2,
    pos: (i32, i32, i32),
    predicted_block_id: BlockRuntimeId,
}

#[derive(Debug, Default)]
struct LocalTerrainPredictionState {
    pending: Vec<PendingTerrainPrediction>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LocalPredictionAdmission {
    interaction_origin_m: [f32; 3],
    target_predicted_block: Option<BlockRuntimeId>,
    target_authoritative_block: Option<BlockRuntimeId>,
    place_predicted_block: Option<BlockRuntimeId>,
    place_authoritative_block: Option<BlockRuntimeId>,
}

struct ClientPredictedTerrainWorld<'a> {
    camera: &'a dyn ClientCameraHitProvider,
}

impl TerrainInteractionWorldViewV2 for ClientPredictedTerrainWorld<'_> {
    fn cell_at(&self, pos: (i32, i32, i32)) -> TerrainInteractionCellV2 {
        presented_block_id_at(self.camera, pos)
            .map(TerrainInteractionCellV2::Loaded)
            .unwrap_or(TerrainInteractionCellV2::NotLoaded)
    }
}

struct ClientTerrainRules {
    allowed_place_block_id: Option<BlockRuntimeId>,
}

impl TerrainInteractionRulesV2 for ClientTerrainRules {
    fn is_solid(&self, block_id: BlockRuntimeId) -> bool {
        block_id.0 != 0
    }

    fn is_supporting(&self, block_id: BlockRuntimeId, _face: ClientBlockFace) -> bool {
        self.is_solid(block_id)
    }

    fn can_place_block(&self, block_id: BlockRuntimeId) -> bool {
        self.allowed_place_block_id
            .is_some_and(|allowed| block_id == allowed)
    }
}

fn select_presented_break_target(
    camera: &dyn ClientCameraHitProvider,
) -> Option<BreakInteractionTarget> {
    let camera_ray = normalized_camera_ray(camera.camera_ray()?)?;
    let hit = raycast_presented_cursor_hit(camera, camera_ray, MAX_RAYCAST_DISTANCE_M)?;

    Some(BreakInteractionTarget { hit, camera_ray })
}

fn select_presented_place_target(
    camera: &dyn ClientCameraHitProvider,
) -> Option<PlaceInteractionTarget> {
    let camera_ray = normalized_camera_ray(camera.camera_ray()?)?;
    let hit = raycast_presented_cursor_hit(camera, camera_ray, MAX_RAYCAST_DISTANCE_M)?;

    let place_pos = add_face_offset(hit.block_pos, hit.face)?;
    let place_cur = presented_block_id_at(camera, place_pos)?;
    if place_cur.0 != 0 {
        return None;
    }

    Some(PlaceInteractionTarget {
        hit,
        camera_ray,
        place_pos,
    })
}

const MAX_PRESENTED_RAYCAST_STEPS: usize = 128;

fn raycast_presented_cursor_hit(
    camera: &dyn ClientCameraHitProvider,
    camera_ray: ClientCameraRay,
    max_distance_m: f32,
) -> Option<ClientCursorHit> {
    if !max_distance_m.is_finite() || max_distance_m <= 0.0 {
        return None;
    }

    let origin = camera_ray.origin;
    let dir = camera_ray.direction;

    let mut pos = (
        floor_to_i32(origin[0])?,
        floor_to_i32(origin[1])?,
        floor_to_i32(origin[2])?,
    );

    let mut hit_face = opposite_dominant_face(dir);

    if presented_block_id_at(camera, pos).is_some_and(|block| block.0 != 0) {
        return Some(ClientCursorHit {
            block_pos: pos,
            face: hit_face,
            distance_m: 0.0,
        });
    }

    let step_x = axis_step(dir[0]);
    let step_y = axis_step(dir[1]);
    let step_z = axis_step(dir[2]);

    let mut t_max_x = axis_initial_t_max(origin[0], dir[0], step_x);
    let mut t_max_y = axis_initial_t_max(origin[1], dir[1], step_y);
    let mut t_max_z = axis_initial_t_max(origin[2], dir[2], step_z);

    let t_delta_x = axis_t_delta(dir[0]);
    let t_delta_y = axis_t_delta(dir[1]);
    let t_delta_z = axis_t_delta(dir[2]);

    for _ in 0..MAX_PRESENTED_RAYCAST_STEPS {
        let (axis, distance_m) = if t_max_x <= t_max_y && t_max_x <= t_max_z {
            (0, t_max_x)
        } else if t_max_y <= t_max_z {
            (1, t_max_y)
        } else {
            (2, t_max_z)
        };

        if !distance_m.is_finite() || distance_m > max_distance_m {
            return None;
        }

        match axis {
            0 => {
                pos.0 = pos.0.checked_add(step_x)?;
                hit_face = face_for_axis_step(0, step_x)?;
                t_max_x += t_delta_x;
            }
            1 => {
                pos.1 = pos.1.checked_add(step_y)?;
                hit_face = face_for_axis_step(1, step_y)?;
                t_max_y += t_delta_y;
            }
            _ => {
                pos.2 = pos.2.checked_add(step_z)?;
                hit_face = face_for_axis_step(2, step_z)?;
                t_max_z += t_delta_z;
            }
        }

        if presented_block_id_at(camera, pos).is_some_and(|block| block.0 != 0) {
            return Some(ClientCursorHit {
                block_pos: pos,
                face: hit_face,
                distance_m: distance_m.max(0.0),
            });
        }
    }

    None
}

fn presented_block_id_at(
    camera: &dyn ClientCameraHitProvider,
    pos: (i32, i32, i32),
) -> Option<BlockRuntimeId> {
    LOCAL_TERRAIN_PREDICTIONS
        .with(|state| {
            state
                .borrow()
                .pending
                .iter()
                .rev()
                .find(|pending| pending.pos == pos)
                .map(|pending| pending.predicted_block_id)
        })
        .or_else(|| camera.predicted_block_id_at(pos))
}

fn floor_to_i32(value: f32) -> Option<i32> {
    if !value.is_finite() || value < i32::MIN as f32 || value > i32::MAX as f32 {
        return None;
    }
    Some(value.floor() as i32)
}

fn axis_step(value: f32) -> i32 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

fn axis_initial_t_max(origin: f32, direction: f32, step: i32) -> f32 {
    if step == 0 {
        return f32::INFINITY;
    }

    let boundary = if step > 0 {
        origin.floor() + 1.0
    } else {
        origin.floor()
    };

    ((boundary - origin) / direction).max(0.0)
}

fn axis_t_delta(direction: f32) -> f32 {
    if direction == 0.0 {
        f32::INFINITY
    } else {
        (1.0 / direction).abs()
    }
}

fn face_for_axis_step(axis: usize, step: i32) -> Option<ClientBlockFace> {
    match (axis, step) {
        (0, 1) => Some(ClientBlockFace::NegX),
        (0, -1) => Some(ClientBlockFace::PosX),
        (1, 1) => Some(ClientBlockFace::NegY),
        (1, -1) => Some(ClientBlockFace::PosY),
        (2, 1) => Some(ClientBlockFace::NegZ),
        (2, -1) => Some(ClientBlockFace::PosZ),
        _ => None,
    }
}

fn opposite_dominant_face(dir: [f32; 3]) -> ClientBlockFace {
    let ax = dir[0].abs();
    let ay = dir[1].abs();
    let az = dir[2].abs();

    if ax >= ay && ax >= az {
        if dir[0] >= 0.0 {
            ClientBlockFace::NegX
        } else {
            ClientBlockFace::PosX
        }
    } else if ay >= az {
        if dir[1] >= 0.0 {
            ClientBlockFace::NegY
        } else {
            ClientBlockFace::PosY
        }
    } else if dir[2] >= 0.0 {
        ClientBlockFace::NegZ
    } else {
        ClientBlockFace::PosZ
    }
}

fn build_break_intent(
    level_id: u32,
    stream_epoch: u32,
    at_input_seq: u32,
    client_view_tick: u64,
    target: BreakInteractionTarget,
) -> TerrainInteractionIntentV2 {
    TerrainInteractionIntentV2 {
        identity: terrain_identity(
            level_id,
            stream_epoch,
            at_input_seq,
            TerrainInteractionKindV2::Break,
        ),
        ray: terrain_ray(target.camera_ray, client_view_tick),
        hit: terrain_hit(target.hit),
        place: None,
    }
}

fn build_place_intent(
    level_id: u32,
    stream_epoch: u32,
    at_input_seq: u32,
    client_view_tick: u64,
    target: PlaceInteractionTarget,
    block_id: BlockRuntimeId,
) -> TerrainInteractionIntentV2 {
    TerrainInteractionIntentV2 {
        identity: terrain_identity(
            level_id,
            stream_epoch,
            at_input_seq,
            TerrainInteractionKindV2::Place,
        ),
        ray: terrain_ray(target.camera_ray, client_view_tick),
        hit: terrain_hit(target.hit),
        place: Some(TerrainPlaceIntentV2 {
            support_block_pos: target.hit.block_pos,
            placement_pos: target.place_pos,
            block_id,
            expected_placement_empty: true,
            expected_support_solid: true,
        }),
    }
}

fn terrain_identity(
    level_id: u32,
    stream_epoch: u32,
    at_input_seq: u32,
    kind: TerrainInteractionKindV2,
) -> TerrainInteractionIdentityV2 {
    TerrainInteractionIdentityV2 {
        level_id,
        stream_epoch,
        input_seq: at_input_seq,
        action_seq: None,
        kind,
        prediction_tx: prediction_tx_for(at_input_seq, kind),
        // The client submit API assigns action_seq after this payload is built,
        // so Vanilla cannot bind same-cell dependencies to final action identity here.
        depends_on: Vec::new(),
    }
}

fn prediction_tx_for(
    at_input_seq: u32,
    kind: TerrainInteractionKindV2,
) -> TerrainPredictionTransactionIdV2 {
    let kind_bit = match kind {
        TerrainInteractionKindV2::Break => 0,
        TerrainInteractionKindV2::Place => 1,
    };
    TerrainPredictionTransactionIdV2((u64::from(at_input_seq) << 1) | kind_bit)
}

fn terrain_ray(camera_ray: ClientCameraRay, client_view_tick: u64) -> TerrainInteractionRayV2 {
    TerrainInteractionRayV2 {
        ray_origin_m: camera_ray.origin,
        ray_dir: camera_ray.direction,
        max_distance_m: MAX_RAYCAST_DISTANCE_M,
        client_view_tick: Some(client_view_tick),
    }
}

fn terrain_hit(hit: ClientCursorHit) -> TerrainInteractionHitV2 {
    TerrainInteractionHitV2 {
        hit_block_pos: hit.block_pos,
        hit_face: hit.face,
        hit_point_m: None,
    }
}

fn validate_local_prediction_admission(
    camera: &dyn ClientCameraHitProvider,
    players: &mut dyn ClientPlayerProvider,
    allowed_place_block_id: Option<BlockRuntimeId>,
    intent: &TerrainInteractionIntentV2,
) -> Result<LocalPredictionAdmission, TerrainInteractionRejectReasonV2> {
    let Some(player_center_pos_m) = local_player_center_pos_m(players) else {
        return Err(TerrainInteractionRejectReasonV2::PolicyDenied);
    };
    let interaction_origin_m = humanoid_interaction_origin_m(player_center_pos_m);
    let mut policy =
        TerrainInteractionValidationPolicyV2::new(interaction_origin_m, MAX_ACTION_REACH_M);
    policy.active_level_id = Some(intent.identity.level_id);
    policy.active_stream_epoch = Some(intent.identity.stream_epoch);
    policy.min_input_seq = Some(intent.identity.input_seq);
    policy.max_input_seq = Some(intent.identity.input_seq);

    let world = ClientPredictedTerrainWorld { camera };
    let rules = ClientTerrainRules {
        allowed_place_block_id,
    };
    match validate_terrain_interaction_v2(&world, &rules, &policy, intent) {
        TerrainInteractionValidationV2::Accepted(_) => {}
        TerrainInteractionValidationV2::Rejected(reason) => return Err(reason),
    }

    let target_pos = intent.hit.hit_block_pos;
    let target_predicted_block = presented_block_id_at(camera, target_pos);
    let target_authoritative_block = camera.authoritative_block_id_at(target_pos);
    let place_pos = intent.place.as_ref().map(|place| place.placement_pos);
    let place_predicted_block = place_pos.and_then(|pos| presented_block_id_at(camera, pos));
    let place_authoritative_block = place_pos.and_then(|pos| camera.authoritative_block_id_at(pos));

    // Local prediction is admitted against the client-presented terrain stream.
    // The server still revalidates and remains authoritative; convergence comes
    // from ActionResult and ordered WorldDelta terrain_rev updates. An
    // authoritative snapshot that has not caught up to a pending local edit is
    // diagnostic context, not a local veto by itself.
    if let Some(reason) = unexplained_authoritative_impossibility(
        intent,
        target_predicted_block,
        target_authoritative_block,
        place_predicted_block,
        place_authoritative_block,
    ) {
        return Err(reason);
    }

    Ok(LocalPredictionAdmission {
        interaction_origin_m,
        target_predicted_block,
        target_authoritative_block,
        place_predicted_block,
        place_authoritative_block,
    })
}

fn unexplained_authoritative_impossibility(
    intent: &TerrainInteractionIntentV2,
    target_predicted: Option<BlockRuntimeId>,
    target_authoritative: Option<BlockRuntimeId>,
    place_predicted: Option<BlockRuntimeId>,
    place_authoritative: Option<BlockRuntimeId>,
) -> Option<TerrainInteractionRejectReasonV2> {
    let target_pos = intent.hit.hit_block_pos;
    let target_auth_air = target_authoritative.is_some_and(|block| block.0 == 0);
    let target_pred_solid = target_predicted.is_some_and(|block| block.0 != 0);
    if target_pred_solid
        && target_auth_air
        && !prediction_mismatch_is_explained(target_pos, target_predicted, target_authoritative)
    {
        return Some(match intent.identity.kind {
            TerrainInteractionKindV2::Break => TerrainInteractionRejectReasonV2::TargetNotSolid,
            TerrainInteractionKindV2::Place => TerrainInteractionRejectReasonV2::SupportNotSolid,
        });
    }

    if let Some(place) = intent.place.as_ref() {
        let place_auth_occupied = place_authoritative.is_some_and(|block| block.0 != 0);
        let place_pred_empty = place_predicted.is_some_and(|block| block.0 == 0);
        if place_pred_empty
            && place_auth_occupied
            && !prediction_mismatch_is_explained(
                place.placement_pos,
                place_predicted,
                place_authoritative,
            )
        {
            return Some(TerrainInteractionRejectReasonV2::PlacementNotEmpty);
        }
    }

    None
}

fn local_player_center_pos_m(players: &mut dyn ClientPlayerProvider) -> Option<[f32; 3]> {
    let mut views = Vec::<ClientPlayerView>::new();
    players.list_players(&mut views);
    views
        .into_iter()
        .find(|view| view.is_local)
        .map(|view| [view.world_pos_m.0, view.world_pos_m.1, view.world_pos_m.2])
}

fn prediction_mismatch_is_explained(
    pos: (i32, i32, i32),
    predicted: Option<BlockRuntimeId>,
    authoritative: Option<BlockRuntimeId>,
) -> bool {
    if predicted == authoritative {
        return true;
    }
    let Some(predicted_block_id) = predicted else {
        return false;
    };
    LOCAL_TERRAIN_PREDICTIONS.with(|state| {
        state
            .borrow()
            .pending
            .iter()
            .any(|pending| pending.pos == pos && pending.predicted_block_id == predicted_block_id)
    })
}

fn remember_local_terrain_predictions(
    action_seq: u32,
    prediction_tx: TerrainPredictionTransactionIdV2,
    predicted: &[ClientPredictedEdit],
) {
    LOCAL_TERRAIN_PREDICTIONS.with(|state| {
        let mut state = state.borrow_mut();
        for edit in predicted {
            state.pending.push(PendingTerrainPrediction {
                action_seq,
                prediction_tx,
                pos: edit.pos,
                predicted_block_id: edit.predicted_block_id,
            });
        }
    });
}

fn prune_local_terrain_predictions(camera: &dyn ClientCameraHitProvider) {
    LOCAL_TERRAIN_PREDICTIONS.with(|state| {
        state.borrow_mut().pending.retain(|pending| {
            let predicted = camera.predicted_block_id_at(pending.pos);
            let authoritative = camera.authoritative_block_id_at(pending.pos);
            predicted != authoritative
        });
    });
}

fn clear_local_terrain_predictions() {
    LOCAL_TERRAIN_PREDICTIONS.with(|state| state.borrow_mut().pending.clear());
}

fn local_pending_prediction_count() -> usize {
    LOCAL_TERRAIN_PREDICTIONS.with(|state| state.borrow().pending.len())
}

fn local_pending_prediction_summary() -> String {
    LOCAL_TERRAIN_PREDICTIONS.with(|state| {
        let state = state.borrow();
        if state.pending.is_empty() {
            return "[]".to_string();
        }

        let mut summary = String::from("[");
        for (index, pending) in state.pending.iter().take(4).enumerate() {
            if index > 0 {
                summary.push_str(", ");
            }
            summary.push_str(&format!(
                "{{action_seq={}, tx={:?}, pos={:?}, predicted={:?}}}",
                pending.action_seq, pending.prediction_tx, pending.pos, pending.predicted_block_id
            ));
        }
        if state.pending.len() > 4 {
            summary.push_str(", ...");
        }
        summary.push(']');
        summary
    })
}

fn log_local_skip(tick: &mut ClientTickApi<'_>, action: ClientMouseButton, reason: &str) {
    let pending_summary = local_pending_prediction_summary();
    let pending_count = local_pending_prediction_count();
    tracing::debug!(
        target: "freven_vanilla_essentials::client::block_interaction",
        action = action_name(action),
        reason,
        pending_terrain_predictions = %pending_summary,
        pending_terrain_prediction_count = pending_count,
        "block interaction not submitted",
    );
    let message = format!(
        "{} interaction not submitted: {reason} pending_terrain_predictions={} pending_terrain_prediction_count={}",
        action_name(action),
        pending_summary,
        pending_count,
    );
    tick.log(LogLevel::Debug, message.clone());
    emit_log(LogLevel::Debug, message);
}

fn log_encode_failure(
    tick: &mut ClientTickApi<'_>,
    action: ClientMouseButton,
    err: &crate::action_payloads::ActionPayloadError,
) {
    tracing::debug!(
        target: "freven_vanilla_essentials::client::block_interaction",
        action = action_name(action),
        error = %err,
        "block interaction payload encode failed",
    );
    tick.log(
        LogLevel::Debug,
        format!(
            "{} interaction not submitted: payload encode failed: {err}",
            action_name(action)
        ),
    );
}

fn log_local_validation_reject(
    tick: &mut ClientTickApi<'_>,
    action: &str,
    intent: &TerrainInteractionIntentV2,
    reason: TerrainInteractionRejectReasonV2,
) {
    tracing::debug!(
        target: "freven_vanilla_essentials::client::block_interaction",
        action,
        reason = ?reason,
        at_input_seq = intent.identity.input_seq,
        target_pos = ?intent.hit.hit_block_pos,
        place_pos = ?intent.place.as_ref().map(|place| place.placement_pos),
        hit_face = ?intent.hit.hit_face,
        ray_origin_m = ?intent.ray.ray_origin_m,
        ray_dir = ?intent.ray.ray_dir,
        prediction_tx = ?intent.identity.prediction_tx,
        depends_on = ?intent.identity.depends_on,
        pending_terrain_predictions = %local_pending_prediction_summary(),
        pending_terrain_prediction_count = local_pending_prediction_count(),
        "block interaction local prediction rejected",
    );
    tick.log(
        LogLevel::Debug,
        format!(
            "{action} interaction not submitted: local v2 prediction validation rejected \
             reason={reason:?} action_seq=missing-pre-submit at_input_seq={} target_pos={:?} \
             place_pos={:?} hit_face={:?} ray_origin_m={:?} ray_dir={:?} prediction_tx={:?} \
             depends_on={:?} pending_terrain_predictions={} pending_terrain_prediction_count={}",
            intent.identity.input_seq,
            intent.hit.hit_block_pos,
            intent.place.as_ref().map(|place| place.placement_pos),
            intent.hit.hit_face,
            intent.ray.ray_origin_m,
            intent.ray.ray_dir,
            intent.identity.prediction_tx,
            intent.identity.depends_on,
            local_pending_prediction_summary(),
            local_pending_prediction_count(),
        ),
    );
}

fn log_local_prediction_accepted(
    tick: &mut ClientTickApi<'_>,
    action: &str,
    action_seq: u32,
    intent: &TerrainInteractionIntentV2,
    admission: LocalPredictionAdmission,
) {
    tracing::debug!(
        target: "freven_vanilla_essentials::client::block_interaction",
        action,
        action_seq,
        at_input_seq = intent.identity.input_seq,
        target_pos = ?intent.hit.hit_block_pos,
        place_pos = ?intent.place.as_ref().map(|place| place.placement_pos),
        hit_face = ?intent.hit.hit_face,
        ray_origin_m = ?intent.ray.ray_origin_m,
        ray_dir = ?intent.ray.ray_dir,
        predicted_target_block = ?admission.target_predicted_block,
        authoritative_target_block = ?admission.target_authoritative_block,
        predicted_place_block = ?admission.place_predicted_block,
        authoritative_place_block = ?admission.place_authoritative_block,
        pending_terrain_predictions = %local_pending_prediction_summary(),
        pending_terrain_prediction_count = local_pending_prediction_count(),
        "block interaction local prediction accepted",
    );

    let message = format!(
        "{action} local prediction accepted: action_seq={action_seq} at_input_seq={} \
         target_pos={:?} place_pos={:?} hit_face={:?} ray_origin_m={:?} ray_dir={:?} \
         client_interaction_origin_m={:?} predicted_target_block={:?} \
         authoritative_target_block={:?} predicted_place_block={:?} \
         authoritative_place_block={:?} prediction_tx={:?} depends_on={:?} \
         pending_terrain_predictions={} pending_terrain_prediction_count={}",
        intent.identity.input_seq,
        intent.hit.hit_block_pos,
        intent.place.as_ref().map(|place| place.placement_pos),
        intent.hit.hit_face,
        intent.ray.ray_origin_m,
        intent.ray.ray_dir,
        admission.interaction_origin_m,
        admission.target_predicted_block,
        admission.target_authoritative_block,
        admission.place_predicted_block,
        admission.place_authoritative_block,
        intent.identity.prediction_tx,
        intent.identity.depends_on,
        local_pending_prediction_summary(),
        local_pending_prediction_count(),
    );
    tick.log(LogLevel::Debug, message.clone());
    emit_log(LogLevel::Debug, message);
}

fn log_submit_failure(tick: &mut ClientTickApi<'_>, action: &str, err: ClientActionSubmitError) {
    tracing::debug!(
        target: "freven_vanilla_essentials::client::block_interaction",
        action,
        error = %err,
        "block interaction submit failed",
    );
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

fn normalized_camera_ray(ray: ClientCameraRay) -> Option<ClientCameraRay> {
    if !vec3_is_finite(ray.origin) || !vec3_is_finite(ray.direction) {
        return None;
    }
    let len = vec3_len(ray.direction);
    if len <= f32::EPSILON {
        return None;
    }
    Some(ClientCameraRay {
        origin: ray.origin,
        direction: [
            ray.direction[0] / len,
            ray.direction[1] / len,
            ray.direction[2] / len,
        ],
    })
}

fn vec3_is_finite(value: [f32; 3]) -> bool {
    value.into_iter().all(f32::is_finite)
}

fn vec3_len(value: [f32; 3]) -> f32 {
    (value[0].mul_add(value[0], value[1].mul_add(value[1], value[2] * value[2]))).sqrt()
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
    use crate::action_payloads::{
        ActionTarget, decode_break_payload_v2, decode_place_payload_v2, encode_break_payload_v1,
        try_encode_break_payload_v2, try_encode_place_payload_v2,
    };
    use freven_avatar_sdk_types::ClientMouseButtonPress;
    use freven_avatar_sdk_types::{
        ClientInputProvider, ClientKeyCode, ClientPlayerProvider, ClientPlayerView,
    };
    use freven_block_api::{
        BlockAuthority, BlockMutationResult, BlockWorldView, BlockWorldViewTerrainAdapter,
        ClientCameraHitProvider, ClientCameraRay, ClientCursorHit, TerrainInteractionRulesV2,
        TerrainInteractionValidationPolicyV2, TerrainInteractionValidationV2,
        validate_terrain_interaction_v2,
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

        fn drain_mouse_button_presses(
            &mut self,
            _owner: &str,
            limit: usize,
        ) -> Vec<ClientMouseButtonPress> {
            let mut presses = Vec::new();
            if limit == 0 {
                return presses;
            }

            if self.left {
                self.left = false;
                presses.push(ClientMouseButtonPress::new(ClientMouseButton::Left));
            }

            if self.right && presses.len() < limit {
                self.right = false;
                presses.push(ClientMouseButtonPress::new(ClientMouseButton::Right));
            }

            presses
        }

        fn consume_key_press(&mut self, _key: ClientKeyCode, _owner: &str) -> bool {
            false
        }
    }

    struct OrderedTestInput {
        presses: Vec<ClientMouseButton>,
    }

    impl OrderedTestInput {
        fn new(presses: impl Into<Vec<ClientMouseButton>>) -> Self {
            Self {
                presses: presses.into(),
            }
        }
    }

    impl ClientInputProvider for OrderedTestInput {
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
            let Some(index) = self.presses.iter().position(|queued| *queued == button) else {
                return false;
            };
            self.presses.remove(index);
            true
        }

        fn drain_mouse_button_presses(
            &mut self,
            _owner: &str,
            limit: usize,
        ) -> Vec<ClientMouseButtonPress> {
            let take = limit.min(self.presses.len());
            self.presses
                .drain(..take)
                .map(ClientMouseButtonPress::new)
                .collect()
        }

        fn consume_key_press(&mut self, _key: ClientKeyCode, _owner: &str) -> bool {
            false
        }
    }

    struct PresentedCamera {
        camera_ray: Option<ClientCameraRay>,
        predicted_hit: Option<ClientCursorHit>,
        authoritative_hit: Option<ClientCursorHit>,
        predicted_blocks: HashMap<(i32, i32, i32), BlockRuntimeId>,
        authoritative_blocks: HashMap<(i32, i32, i32), BlockRuntimeId>,
    }

    impl PresentedCamera {
        fn new(predicted_hit: ClientCursorHit) -> Self {
            Self {
                camera_ray: Some(ClientCameraRay {
                    origin: [0.5, 1.62, 0.5],
                    direction: [1.0, 0.0, 0.0],
                }),
                predicted_hit: Some(predicted_hit),
                authoritative_hit: Some(predicted_hit),
                predicted_blocks: HashMap::new(),
                authoritative_blocks: HashMap::new(),
            }
        }

        fn with_block(mut self, pos: (i32, i32, i32), block_id: u32) -> Self {
            self.predicted_blocks.insert(pos, BlockRuntimeId(block_id));
            self.authoritative_blocks
                .insert(pos, BlockRuntimeId(block_id));
            self
        }

        fn with_predicted_block(mut self, pos: (i32, i32, i32), block_id: u32) -> Self {
            self.predicted_blocks.insert(pos, BlockRuntimeId(block_id));
            self
        }

        fn with_authoritative_block(mut self, pos: (i32, i32, i32), block_id: u32) -> Self {
            self.authoritative_blocks
                .insert(pos, BlockRuntimeId(block_id));
            self
        }

        fn with_authoritative_hit(mut self, hit: Option<ClientCursorHit>) -> Self {
            self.authoritative_hit = hit;
            self
        }
    }

    impl ClientCameraHitProvider for PresentedCamera {
        fn camera_ray(&self) -> Option<ClientCameraRay> {
            self.camera_ray
        }

        fn authoritative_cursor_hit(&self, _max_distance_m: f32) -> Option<ClientCursorHit> {
            self.authoritative_hit
        }

        fn predicted_cursor_hit(&self, _max_distance_m: f32) -> Option<ClientCursorHit> {
            self.predicted_hit
        }

        fn predicted_block_id_at(&self, pos: (i32, i32, i32)) -> Option<BlockRuntimeId> {
            Some(
                *self
                    .predicted_blocks
                    .get(&pos)
                    .unwrap_or(&BlockRuntimeId(0)),
            )
        }

        fn authoritative_block_id_at(&self, pos: (i32, i32, i32)) -> Option<BlockRuntimeId> {
            Some(
                *self
                    .authoritative_blocks
                    .get(&pos)
                    .unwrap_or(&BlockRuntimeId(0)),
            )
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
        fn list_players(&self, out: &mut Vec<ClientPlayerView>) {
            out.push(ClientPlayerView {
                player_id: 7,
                world_pos_m: (0.5, 0.9, 0.5),
                is_local: true,
            });
        }

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

    struct TestPhysics {
        pos: [f32; 3],
    }

    impl Default for TestPhysics {
        fn default() -> Self {
            Self {
                pos: [0.5, 0.9, 0.5],
            }
        }
    }

    impl CharacterPhysicsQuery for TestPhysics {
        fn player_position(&self, _player_id: u64) -> Option<[f32; 3]> {
            Some(self.pos)
        }
    }

    struct TestTerrainRules<'a> {
        world: &'a dyn BlockWorldView,
    }

    impl TerrainInteractionRulesV2 for TestTerrainRules<'_> {
        fn is_solid(&self, block_id: BlockRuntimeId) -> bool {
            self.world.is_solid(block_id)
        }
    }

    fn ensure_action_kinds() {
        clear_local_terrain_predictions();
        let _ = crate::VANILLA_ACTION_KINDS.get_or_init(|| crate::VanillaActionKinds {
            break_kind: ActionKindId(1),
            place_kind: ActionKindId(2),
        });
    }

    fn test_identity(
        kind: TerrainInteractionKindV2,
        input_seq: u32,
        action_seq: u32,
    ) -> TerrainInteractionIdentityV2 {
        TerrainInteractionIdentityV2 {
            level_id: 1,
            stream_epoch: 1,
            input_seq,
            action_seq: Some(action_seq),
            kind,
            prediction_tx: TerrainPredictionTransactionIdV2(u64::from(action_seq)),
            depends_on: Vec::new(),
        }
    }

    fn server_break_intent(
        target: (i32, i32, i32),
        distance_m: f32,
        input_seq: u32,
        action_seq: u32,
    ) -> TerrainInteractionIntentV2 {
        TerrainInteractionIntentV2 {
            identity: test_identity(TerrainInteractionKindV2::Break, input_seq, action_seq),
            ray: TerrainInteractionRayV2 {
                ray_origin_m: [0.5, 1.62, 0.5],
                ray_dir: [1.0, 0.0, 0.0],
                max_distance_m: distance_m,
                client_view_tick: Some(1),
            },
            hit: TerrainInteractionHitV2 {
                hit_block_pos: target,
                hit_face: ClientBlockFace::NegX,
                hit_point_m: Some([target.0 as f32, 1.62, 0.5]),
            },
            place: None,
        }
    }

    fn server_place_intent(
        support: (i32, i32, i32),
        placement: (i32, i32, i32),
        block_id: BlockRuntimeId,
        input_seq: u32,
        action_seq: u32,
    ) -> TerrainInteractionIntentV2 {
        TerrainInteractionIntentV2 {
            identity: test_identity(TerrainInteractionKindV2::Place, input_seq, action_seq),
            ray: TerrainInteractionRayV2 {
                ray_origin_m: [0.5, 1.62, 0.5],
                ray_dir: [1.0, 0.0, 0.0],
                max_distance_m: 5.0,
                client_view_tick: Some(1),
            },
            hit: TerrainInteractionHitV2 {
                hit_block_pos: support,
                hit_face: ClientBlockFace::NegX,
                hit_point_m: Some([support.0 as f32, 1.62, 0.5]),
            },
            place: Some(TerrainPlaceIntentV2 {
                support_block_pos: support,
                placement_pos: placement,
                block_id,
                expected_placement_empty: true,
                expected_support_solid: true,
            }),
        }
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
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((2, 1, 0), 1);
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
        let payload = decode_break_payload_v2(&req.payload).expect("decode break");
        assert_eq!(payload.identity.level_id, 1);
        assert_eq!(payload.identity.stream_epoch, 1);
        assert_eq!(payload.identity.input_seq, 42);
        assert_eq!(payload.identity.action_seq, None);
        assert_eq!(payload.identity.depends_on, Vec::new());
        assert_eq!(payload.hit.hit_block_pos, (2, 1, 0));
        assert_eq!(payload.hit.hit_face, ClientBlockFace::NegX);
        assert_eq!(payload.ray.ray_origin_m, [0.5, 1.62, 0.5]);
        assert_eq!(payload.ray.ray_dir, [1.0, 0.0, 0.0]);
        assert_eq!(payload.ray.max_distance_m, MAX_RAYCAST_DISTANCE_M);
        assert_eq!(payload.ray.client_view_tick, Some(7));
        assert_eq!(payload.hit.hit_point_m, None);
        assert_eq!(
            req.predicted,
            vec![ClientPredictedEdit::clear_block((2, 1, 0))]
        );
    }

    #[test]
    fn pre_submit_identity_defers_action_seq_and_same_cell_dependencies() {
        let target = BreakInteractionTarget {
            hit: ClientCursorHit {
                block_pos: (2, 1, 0),
                face: ClientBlockFace::NegX,
                distance_m: 1.5,
            },
            camera_ray: ClientCameraRay {
                origin: [0.5, 1.62, 0.5],
                direction: [1.0, 0.0, 0.0],
            },
        };

        let intent = build_break_intent(1, 1, 42, 7, target);

        assert_eq!(intent.identity.action_seq, None);
        assert_eq!(intent.identity.depends_on, Vec::new());
        assert_eq!(
            intent.identity.prediction_tx,
            TerrainPredictionTransactionIdV2(84)
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
            block_pos: (3, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 2.5,
        })
        .with_block((2, 1, 0), 0)
        .with_block((3, 1, 0), 1);
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
        let payload = decode_break_payload_v2(&req.payload).expect("decode break");
        assert_eq!(payload.hit.hit_block_pos, (3, 1, 0));
        assert_eq!(
            req.predicted,
            vec![ClientPredictedEdit::clear_block((3, 1, 0))]
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
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 2.0,
        })
        .with_block((1, 1, 0), 0)
        .with_block((2, 1, 0), 1)
        .with_block((3, 1, 0), 1);
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
        let payload = decode_place_payload_v2(&req.payload).expect("decode place");
        assert_eq!(payload.hit.hit_block_pos, (2, 1, 0));
        assert_eq!(payload.hit.hit_face, ClientBlockFace::NegX);
        let place = payload.place.expect("place intent");
        assert_eq!(place.support_block_pos, (2, 1, 0));
        assert_eq!(place.placement_pos, (1, 1, 0));
        assert_eq!(place.block_id, BlockRuntimeId(3));
        assert!(place.expected_placement_empty);
        assert!(place.expected_support_solid);
        assert_eq!(
            req.predicted,
            vec![ClientPredictedEdit {
                pos: (1, 1, 0),
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
            block_pos: (1, 1, 0),
            face: ClientBlockFace::PosY,
            distance_m: 2.0,
        })
        .with_block((1, 1, 0), 0)
        .with_block((10, 2, 30), 0);
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
    fn rapid_break_spam_stale_predicted_only_target_does_not_flash_prediction() {
        ensure_action_kinds();

        // A predicted-only solid with no remembered pending edit is not a
        // valid dependency. Suppress it locally so a stale visual target does
        // not flash another prediction before the server can reject it.
        let mut services = NoopServices;
        let mut input = TestInput {
            left: true,
            right: false,
        };
        let mut camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_predicted_block((2, 1, 0), 1)
        .with_authoritative_block((2, 1, 0), 0);
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
            let mut tick = ClientTickApi::new(13, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert!(interaction.requests.is_empty());
    }

    #[test]
    fn rapid_place_spam_authoritative_occupied_cell_does_not_flash_prediction() {
        ensure_action_kinds();

        let mut services = NoopServices;
        let mut input = TestInput {
            left: false,
            right: true,
        };
        let mut camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((2, 1, 0), 1)
        .with_predicted_block((1, 1, 0), 0)
        .with_authoritative_block((1, 1, 0), 2);
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
            let mut tick = ClientTickApi::new(14, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert!(interaction.requests.is_empty());
    }

    #[test]
    fn valid_rapid_break_chain_remains_predicted() {
        ensure_action_kinds();

        // The second break targets the client-presented stream after the first
        // local prediction. The authoritative cursor still sees the first
        // block, but authoritative lag alone must not suppress the dependent
        // prediction; server authority converges through ActionResult plus
        // ordered WorldDelta terrain_rev updates.
        let mut services = NoopServices;
        let mut input = TestInput {
            left: true,
            right: false,
        };
        let mut first_camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((2, 1, 0), 1)
        .with_block((3, 1, 0), 1);
        let mut interaction = RecordingInteraction::default();
        let mut players = NoopPlayers;

        {
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut first_camera,
                &mut interaction,
                &mut players,
            );
            let mut tick = ClientTickApi::new(15, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        let mut input = TestInput {
            left: true,
            right: false,
        };
        let mut second_camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (3, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 2.5,
        })
        .with_predicted_block((2, 1, 0), 0)
        .with_authoritative_block((2, 1, 0), 1)
        .with_block((3, 1, 0), 1)
        .with_authoritative_hit(Some(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        }));

        {
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut second_camera,
                &mut interaction,
                &mut players,
            );
            let mut tick = ClientTickApi::new(16, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert_eq!(interaction.requests.len(), 2);
        assert_eq!(
            interaction.requests[0].predicted,
            vec![ClientPredictedEdit::clear_block((2, 1, 0))]
        );
        assert_eq!(
            interaction.requests[1].predicted,
            vec![ClientPredictedEdit::clear_block((3, 1, 0))]
        );
    }

    #[test]
    fn ordered_same_tick_left_then_right_submits_both_actions() {
        ensure_action_kinds();

        let mut services = NoopServices;
        let mut input =
            OrderedTestInput::new(vec![ClientMouseButton::Left, ClientMouseButton::Right]);
        let mut camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((1, 1, 0), 0)
        .with_block((2, 1, 0), 1)
        .with_block((3, 1, 0), 1);
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
            let mut tick = ClientTickApi::new(21, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert_eq!(interaction.requests.len(), 2);
        assert_eq!(
            interaction.requests[0].action_kind_id,
            break_action_kind_id()
        );
        assert_eq!(
            interaction.requests[1].action_kind_id,
            place_action_kind_id()
        );
        assert_eq!(
            interaction.requests[0].predicted,
            vec![ClientPredictedEdit::clear_block((2, 1, 0))]
        );
        assert_eq!(
            interaction.requests[1].predicted,
            vec![ClientPredictedEdit {
                pos: (2, 1, 0),
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
    }

    #[test]
    fn ordered_same_tick_double_right_submits_two_place_actions() {
        ensure_action_kinds();

        let mut services = NoopServices;
        let mut input =
            OrderedTestInput::new(vec![ClientMouseButton::Right, ClientMouseButton::Right]);
        let mut camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((0, 1, 0), 0)
        .with_block((1, 1, 0), 0)
        .with_block((2, 1, 0), 1);
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
            let mut tick = ClientTickApi::new(22, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert_eq!(interaction.requests.len(), 2);
        assert_eq!(
            interaction.requests[0].action_kind_id,
            place_action_kind_id()
        );
        assert_eq!(
            interaction.requests[1].action_kind_id,
            place_action_kind_id()
        );
        assert_eq!(
            interaction.requests[0].predicted,
            vec![ClientPredictedEdit {
                pos: (1, 1, 0),
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
        assert_eq!(
            interaction.requests[1].predicted,
            vec![ClientPredictedEdit {
                pos: (0, 1, 0),
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
    }

    #[test]
    fn ordered_same_tick_right_then_left_preserves_order() {
        ensure_action_kinds();

        let mut services = NoopServices;
        let mut input =
            OrderedTestInput::new(vec![ClientMouseButton::Right, ClientMouseButton::Left]);
        let mut camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((1, 1, 0), 0)
        .with_block((2, 1, 0), 1);
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
            let mut tick = ClientTickApi::new(23, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert_eq!(interaction.requests.len(), 2);
        assert_eq!(
            interaction.requests[0].action_kind_id,
            place_action_kind_id()
        );
        assert_eq!(
            interaction.requests[1].action_kind_id,
            break_action_kind_id()
        );
    }

    #[test]
    fn break_then_place_into_just_predicted_empty_cell_submits() {
        ensure_action_kinds();

        let mut services = NoopServices;
        let mut input = TestInput {
            left: true,
            right: false,
        };
        let mut break_camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((2, 1, 0), 1)
        .with_block((3, 1, 0), 1);
        let mut interaction = RecordingInteraction::default();
        let mut players = NoopPlayers;

        {
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut break_camera,
                &mut interaction,
                &mut players,
            );
            let mut tick = ClientTickApi::new(19, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        let mut input = TestInput {
            left: false,
            right: true,
        };
        let mut place_camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (3, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 2.5,
        })
        .with_predicted_block((2, 1, 0), 0)
        .with_authoritative_block((2, 1, 0), 1)
        .with_block((3, 1, 0), 1)
        .with_authoritative_hit(Some(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        }));

        {
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut place_camera,
                &mut interaction,
                &mut players,
            );
            let mut tick = ClientTickApi::new(20, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert_eq!(interaction.requests.len(), 2);
        assert_eq!(
            interaction.requests[0].predicted,
            vec![ClientPredictedEdit::clear_block((2, 1, 0))]
        );
        assert_eq!(
            interaction.requests[1].action_kind_id,
            place_action_kind_id()
        );
        let payload = decode_place_payload_v2(&interaction.requests[1].payload)
            .expect("decode dependent place");
        assert_eq!(payload.hit.hit_block_pos, (3, 1, 0));
        let place = payload.place.expect("place intent");
        assert_eq!(place.support_block_pos, (3, 1, 0));
        assert_eq!(place.placement_pos, (2, 1, 0));
        assert_eq!(
            interaction.requests[1].predicted,
            vec![ClientPredictedEdit {
                pos: (2, 1, 0),
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
    }

    #[test]
    fn valid_rapid_place_chain_remains_predicted() {
        ensure_action_kinds();

        // Rapid RMB can use a just-predicted placed block as the next support.
        // Client prediction is allowed to target presented/pending state; the
        // server still validates the submitted intent on authoritative state.
        let mut services = NoopServices;
        let mut input = TestInput {
            left: false,
            right: true,
        };
        let mut first_camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((1, 1, 0), 0)
        .with_block((2, 1, 0), 1);
        let mut interaction = RecordingInteraction::default();
        let mut players = NoopPlayers;

        {
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut first_camera,
                &mut interaction,
                &mut players,
            );
            let mut tick = ClientTickApi::new(17, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        let mut input = TestInput {
            left: false,
            right: true,
        };
        let mut second_camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (1, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 0.5,
        })
        .with_block((0, 1, 0), 0)
        .with_predicted_block((1, 1, 0), 3)
        .with_authoritative_block((1, 1, 0), 0)
        .with_block((2, 1, 0), 1)
        .with_authoritative_hit(Some(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        }));

        {
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut second_camera,
                &mut interaction,
                &mut players,
            );
            let mut tick = ClientTickApi::new(18, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert_eq!(interaction.requests.len(), 2);
        assert_eq!(
            interaction.requests[0].predicted,
            vec![ClientPredictedEdit {
                pos: (1, 1, 0),
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
        assert_eq!(
            interaction.requests[1].predicted,
            vec![ClientPredictedEdit {
                pos: (0, 1, 0),
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
    }

    #[test]
    fn authoritative_block_id_mismatch_alone_does_not_reject_presented_valid_break() {
        ensure_action_kinds();

        let intent = server_break_intent((2, 1, 0), 1.5, 42, 1);
        let camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_predicted_block((2, 1, 0), 3)
        .with_authoritative_block((2, 1, 0), 1);
        let mut players = NoopPlayers;

        let client_validation =
            validate_local_prediction_admission(&camera, &mut players, None, &intent);

        assert!(
            client_validation.is_ok(),
            "#79-style predicted-vs-authoritative block-id mismatch alone must not reject a \
             presented-valid local prediction: {client_validation:?}"
        );
    }

    #[test]
    fn client_server_v2_validation_parity_for_same_intent_snapshot() {
        ensure_action_kinds();

        let intent = server_break_intent((2, 1, 0), 1.5, 42, 1);
        let authority = TestAuthority::default().with_block((2, 1, 0), 1);
        let world = BlockWorldViewTerrainAdapter::new(&authority);
        let rules = TestTerrainRules { world: &authority };
        let policy = TerrainInteractionValidationPolicyV2::new([0.5, 1.62, 0.5], 5.0);
        let server_validation = validate_terrain_interaction_v2(&world, &rules, &policy, &intent);

        let camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((2, 1, 0), 1);
        let mut players = NoopPlayers;
        let client_validation =
            validate_local_prediction_admission(&camera, &mut players, None, &intent);

        assert!(matches!(
            server_validation,
            TerrainInteractionValidationV2::Accepted(_)
        ));
        assert!(client_validation.is_ok());
    }

    #[test]
    fn local_v2_prediction_reject_surfaces_concrete_reason() {
        ensure_action_kinds();

        let intent = server_break_intent((2, 1, 0), 1.5, 42, 1);
        let camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((1, 1, 0), 1)
        .with_block((2, 1, 0), 1);
        let mut players = NoopPlayers;

        let client_validation =
            validate_local_prediction_admission(&camera, &mut players, None, &intent);

        assert_eq!(
            client_validation,
            Err(TerrainInteractionRejectReasonV2::Occluded)
        );
    }

    #[test]
    fn server_authoritative_validation_rejects_predicted_only_targets() {
        let physics = TestPhysics::default();
        let break_payload =
            try_encode_break_payload_v2(&server_break_intent((2, 1, 0), 1.5, 42, 1))
                .expect("encode break v2");
        let break_cmd = ActionCmdView {
            action_kind: ActionKindId(1),
            level_id: 1,
            stream_epoch: 1,
            seq: 1,
            at_input_seq: 42,
            payload: &break_payload,
        };
        let mut services = NoopServices;
        let mut break_authority = TestAuthority::default().with_block((2, 1, 0), 0);
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

        let place_payload = try_encode_place_payload_v2(&server_place_intent(
            (2, 1, 0),
            (1, 1, 0),
            BlockRuntimeId(3),
            43,
            2,
        ))
        .expect("encode place v2");
        let place_cmd = ActionCmdView {
            action_kind: ActionKindId(2),
            level_id: 1,
            stream_epoch: 1,
            seq: 2,
            at_input_seq: 43,
            payload: &place_payload,
        };
        let mut place_authority = TestAuthority::default()
            .with_block((1, 1, 0), 0)
            .with_block((2, 1, 0), 0);
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
    fn server_accepts_valid_v2_break_with_center_to_eye_origin() {
        let physics = TestPhysics::default();
        let payload = try_encode_break_payload_v2(&server_break_intent((2, 1, 0), 1.5, 42, 1))
            .expect("encode break v2");
        let cmd = ActionCmdView {
            action_kind: ActionKindId(1),
            level_id: 1,
            stream_epoch: 1,
            seq: 1,
            at_input_seq: 42,
            payload: &payload,
        };
        let mut services = NoopServices;
        let mut authority = TestAuthority::default().with_block((2, 1, 0), 1);
        let mut ctx = ActionContext::new(
            None,
            Some(&mut authority),
            Some(&physics),
            Some(&mut services),
            7,
            42,
        );

        let mut handler = crate::actions::r#break::BreakActionHandler;
        assert_eq!(handler.handle(&mut ctx, &cmd), ActionOutcome::Applied);
        assert_eq!(authority.block(2, 1, 0), Some(BlockRuntimeId(0)));
    }

    #[test]
    fn server_accepts_valid_v2_place_with_center_to_eye_origin() {
        let physics = TestPhysics::default();
        let payload = try_encode_place_payload_v2(&server_place_intent(
            (2, 1, 0),
            (1, 1, 0),
            BlockRuntimeId(3),
            42,
            1,
        ))
        .expect("encode place v2");
        let cmd = ActionCmdView {
            action_kind: ActionKindId(2),
            level_id: 1,
            stream_epoch: 1,
            seq: 1,
            at_input_seq: 42,
            payload: &payload,
        };
        let mut services = NoopServices;
        let mut authority = TestAuthority::default()
            .with_block((1, 1, 0), 0)
            .with_block((2, 1, 0), 1);
        let mut ctx = ActionContext::new(
            None,
            Some(&mut authority),
            Some(&physics),
            Some(&mut services),
            7,
            42,
        );

        let mut handler = crate::actions::place::PlaceActionHandler;
        assert_eq!(handler.handle(&mut ctx, &cmd), ActionOutcome::Applied);
        assert_eq!(authority.block(1, 1, 0), Some(BlockRuntimeId(3)));
    }

    #[test]
    fn old_center_plus_full_eye_height_origin_rejects_valid_v2_break_geometry() {
        let center_pos_m = TestPhysics::default().pos;
        let old_buggy_origin_m = [center_pos_m[0], center_pos_m[1] + 1.62, center_pos_m[2]];
        let intent = server_break_intent((2, 1, 0), 1.5, 42, 1);
        let authority = TestAuthority::default().with_block((2, 1, 0), 1);
        let world = BlockWorldViewTerrainAdapter::new(&authority);
        let rules = TestTerrainRules { world: &authority };
        let policy = TerrainInteractionValidationPolicyV2::new(old_buggy_origin_m, 5.0);

        let validation = validate_terrain_interaction_v2(&world, &rules, &policy, &intent);

        assert!(
            matches!(validation, TerrainInteractionValidationV2::Rejected(_)),
            "old center + full eye-height origin should miss/reject, got {validation:?}"
        );
    }

    #[test]
    fn server_rejects_out_of_reach_v2_break() {
        let physics = TestPhysics::default();
        let payload = try_encode_break_payload_v2(&server_break_intent((7, 1, 0), 6.5, 42, 1))
            .expect("encode break v2");
        let cmd = ActionCmdView {
            action_kind: ActionKindId(1),
            level_id: 1,
            stream_epoch: 1,
            seq: 1,
            at_input_seq: 42,
            payload: &payload,
        };
        let mut services = NoopServices;
        let mut authority = TestAuthority::default().with_block((7, 1, 0), 1);
        let mut ctx = ActionContext::new(
            None,
            Some(&mut authority),
            Some(&physics),
            Some(&mut services),
            7,
            42,
        );

        let mut handler = crate::actions::r#break::BreakActionHandler;
        assert_eq!(handler.handle(&mut ctx, &cmd), ActionOutcome::Rejected);
        assert_eq!(authority.block(7, 1, 0), Some(BlockRuntimeId(1)));
    }

    #[test]
    fn server_rejects_occluded_v2_break() {
        let physics = TestPhysics::default();
        let payload = try_encode_break_payload_v2(&server_break_intent((2, 1, 0), 1.5, 42, 1))
            .expect("encode break v2");
        let cmd = ActionCmdView {
            action_kind: ActionKindId(1),
            level_id: 1,
            stream_epoch: 1,
            seq: 1,
            at_input_seq: 42,
            payload: &payload,
        };
        let mut services = NoopServices;
        let mut authority = TestAuthority::default()
            .with_block((1, 1, 0), 1)
            .with_block((2, 1, 0), 1);
        let mut ctx = ActionContext::new(
            None,
            Some(&mut authority),
            Some(&physics),
            Some(&mut services),
            7,
            42,
        );

        let mut handler = crate::actions::r#break::BreakActionHandler;
        assert_eq!(handler.handle(&mut ctx, &cmd), ActionOutcome::Rejected);
        assert_eq!(authority.block(2, 1, 0), Some(BlockRuntimeId(1)));
    }

    #[test]
    fn server_rejects_occupied_v2_place() {
        let physics = TestPhysics::default();
        let payload = try_encode_place_payload_v2(&server_place_intent(
            (2, 1, 0),
            (1, 1, 0),
            BlockRuntimeId(3),
            42,
            1,
        ))
        .expect("encode place v2");
        let cmd = ActionCmdView {
            action_kind: ActionKindId(2),
            level_id: 1,
            stream_epoch: 1,
            seq: 1,
            at_input_seq: 42,
            payload: &payload,
        };
        let mut services = NoopServices;
        let mut authority = TestAuthority::default()
            .with_block((1, 1, 0), 2)
            .with_block((2, 1, 0), 1);
        let mut ctx = ActionContext::new(
            None,
            Some(&mut authority),
            Some(&physics),
            Some(&mut services),
            7,
            42,
        );

        let mut handler = crate::actions::place::PlaceActionHandler;
        assert_eq!(handler.handle(&mut ctx, &cmd), ActionOutcome::Rejected);
        assert_eq!(authority.block(1, 1, 0), Some(BlockRuntimeId(2)));
    }

    #[test]
    fn server_rejects_stale_v1_payload_on_rc10_path() {
        let physics = TestPhysics::default();
        let payload = encode_break_payload_v1(
            2,
            ActionTarget {
                pos: (2, 1, 0),
                face: 0,
            },
        );
        let cmd = ActionCmdView {
            action_kind: ActionKindId(1),
            level_id: 1,
            stream_epoch: 1,
            seq: 1,
            at_input_seq: 42,
            payload: &payload,
        };
        let mut services = NoopServices;
        let mut authority = TestAuthority::default().with_block((2, 1, 0), 1);
        let mut ctx = ActionContext::new(
            None,
            Some(&mut authority),
            Some(&physics),
            Some(&mut services),
            7,
            42,
        );

        let mut handler = crate::actions::r#break::BreakActionHandler;
        assert_eq!(handler.handle(&mut ctx, &cmd), ActionOutcome::Rejected);
        assert_eq!(authority.block(2, 1, 0), Some(BlockRuntimeId(1)));
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
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 2.0,
        })
        .with_block((1, 1, 0), 0)
        .with_block((2, 1, 0), 1);
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
            block_pos: (1, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((1, 1, 0), 3)
        .with_block((2, 1, 0), 1);

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
                pos: (1, 1, 0),
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
        assert_eq!(
            interaction.requests[1].predicted,
            vec![ClientPredictedEdit::clear_block((1, 1, 0))]
        );
    }
}
