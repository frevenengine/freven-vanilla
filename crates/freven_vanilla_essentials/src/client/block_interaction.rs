use std::sync::Arc;

use crate::action_payloads::{try_encode_break_payload_v2, try_encode_place_payload_v2};
use crate::actions::targeting::{
    MAX_ACTION_REACH_M, bounded_client_aim_origin_m, humanoid_collision_half_extents_m,
    humanoid_interaction_origin_m,
};
use crate::{STONE_KEY, break_action_kind_id, place_action_kind_id};
use freven_avatar_api::{ClientApi, ClientLifecycleHandler, ClientTickApi};
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
use freven_block_sdk_types::{BlockRuntimeId, BlockShapeBox};
use freven_mod_api::{LogLevel, emit_log};
use freven_world_api::{
    ClientActionRequest, ClientActionSubmitError, Services, WorldServiceRequest,
    WorldServiceResponse,
};

const OWNER: &str = "freven.vanilla.essentials:block_interaction";
const MAX_RAYCAST_DISTANCE_M: f32 = 5.0;
const MAX_MOUSE_PRESSES_PER_TICK: usize = 8;
pub fn start_client(api: &mut ClientApi<'_>) {
    let _ = api.input.bind_mouse_button(ClientMouseButton::Left, OWNER);
    let _ = api.input.bind_mouse_button(ClientMouseButton::Right, OWNER);
}

#[cfg(test)]
fn tick_client(tick: &mut ClientTickApi<'_>) {
    // Test-only stateless harness for legacy single-tick unit coverage.
    // Runtime registration uses `BlockInteractionClientState` below so
    // cross-tick intent aggregation is owned by the client session.
    let mut state = BlockInteractionClientState::default();
    state.tick_client_inner(tick, false);
}

#[derive(Default)]
pub struct BlockInteractionClientState {
    pending_place: Option<PreparedBlockAction>,
}

impl ClientLifecycleHandler for BlockInteractionClientState {
    fn on_start_client(&mut self, api: &mut ClientApi<'_>) {
        self.pending_place = None;
        start_client(api);
    }

    fn on_tick_client(&mut self, tick: &mut ClientTickApi<'_>) {
        self.tick_client_inner(tick, true);
    }
}

impl BlockInteractionClientState {
    fn tick_client_inner(&mut self, tick: &mut ClientTickApi<'_>, defer_isolated_place: bool) {
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

        let had_pending_place_at_tick_start = self.pending_place.is_some();
        let mut pending_place = self.pending_place.take();
        let mut batch_predicted_edits = Vec::<ClientPredictedEdit>::new();
        if let Some(pending) = pending_place.as_ref() {
            batch_predicted_edits.extend(pending.predicted.iter().copied());
        }

        let mut prepared_actions = Vec::<PreparedBlockAction>::new();

        for press in presses {
            tracing::debug!(
                target: "freven_vanilla_essentials::client::block_interaction",
                action = ?press.button,
                "handling block interaction mouse press",
            );

            let Some(prepared) =
                prepare_mouse_press_action(tick, press.button, &batch_predicted_edits)
            else {
                continue;
            };

            if let Some(previous_place) = pending_place.take() {
                if let Some(placed_edit) =
                    net_neutral_place_break_predicted_edit(&previous_place, &prepared)
                    && remove_batch_predicted_edit(&mut batch_predicted_edits, placed_edit)
                {
                    log_coalesced_net_neutral_bounded_place_break(tick, &prepared);
                    continue;
                }

                prepared_actions.push(previous_place);
            }

            if coalesce_net_neutral_same_frame_place_break(
                &mut prepared_actions,
                &mut batch_predicted_edits,
                &prepared,
            ) {
                log_coalesced_net_neutral_same_frame_place_break(tick, &prepared);
                continue;
            }

            batch_predicted_edits.extend(prepared.predicted.iter().copied());
            prepared_actions.push(prepared);
        }

        if let Some(previous_place) = pending_place.take() {
            prepared_actions.push(previous_place);
        }

        if defer_isolated_place
            && !had_pending_place_at_tick_start
            && prepared_actions.len() == 1
            && prepared_actions[0].kind == PreparedBlockActionKind::Place
        {
            tracing::debug!(
                target: "freven_vanilla_essentials::client::block_interaction",
                at_input_seq = prepared_actions[0].intent.identity.input_seq,
                place_pos = ?prepared_actions[0].predicted.first().map(|edit| edit.pos),
                "holding isolated place intent for bounded cancel window",
            );
            self.pending_place = prepared_actions.pop();
        }

        for prepared in prepared_actions {
            if !submit_prepared_block_action(tick, prepared) {
                break;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreparedBlockActionKind {
    Break,
    Place,
}

struct PreparedBlockAction {
    kind: PreparedBlockActionKind,
    at_input_seq: u32,
    payload: Vec<u8>,
    predicted: Vec<ClientPredictedEdit>,
    intent: TerrainInteractionIntentV2,
    admission: LocalPredictionAdmission,
}

fn prepare_mouse_press_action(
    tick: &mut ClientTickApi<'_>,
    action: ClientMouseButton,
    batch_predicted_edits: &[ClientPredictedEdit],
) -> Option<PreparedBlockAction> {
    // We only allow submitting actions when the client has an active stream.
    let Some((level_id, stream_epoch)) = tick.client.interaction.active_stream() else {
        log_local_skip(tick, action, "no active world stream");
        return None;
    };

    let at_input_seq = tick.client.interaction.next_input_seq();
    let client_view_tick = tick.tick;

    match action {
        ClientMouseButton::Left => {
            let Some(target) =
                select_presented_break_target(tick.client.camera, batch_predicted_edits)
            else {
                log_local_skip(tick, action, missing_target_reason(action));
                return None;
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
                batch_predicted_edits,
            ) {
                Ok(admission) => admission,
                Err(reason) => {
                    log_local_validation_reject(tick, "break", &intent, reason);
                    return None;
                }
            };

            let payload = match try_encode_break_payload_v2(&intent) {
                Ok(payload) => payload,
                Err(err) => {
                    log_encode_failure(tick, action, &err);
                    return None;
                }
            };

            Some(PreparedBlockAction {
                kind: PreparedBlockActionKind::Break,
                at_input_seq,
                payload,
                predicted: vec![ClientPredictedEdit::clear_block(target.hit.block_pos)],
                intent,
                admission,
            })
        }

        ClientMouseButton::Right => {
            let Some(target) =
                select_presented_place_target(tick.client.camera, batch_predicted_edits)
            else {
                log_local_skip(tick, action, missing_target_reason(action));
                return None;
            };
            let Some(place_block_id) =
                query_block_id_via_block_service(tick.client.services, STONE_KEY)
            else {
                log_local_skip(
                    tick,
                    action,
                    "place block id is not available in the client runtime",
                );
                return None;
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
                batch_predicted_edits,
            ) {
                Ok(admission) => admission,
                Err(reason) => {
                    log_local_validation_reject(tick, "place", &intent, reason);
                    return None;
                }
            };

            let payload = match try_encode_place_payload_v2(&intent) {
                Ok(payload) => payload,
                Err(err) => {
                    log_encode_failure(tick, action, &err);
                    return None;
                }
            };

            Some(PreparedBlockAction {
                kind: PreparedBlockActionKind::Place,
                at_input_seq,
                payload,
                predicted: vec![ClientPredictedEdit {
                    pos: target.place_pos,
                    predicted_block_id: place_block_id,
                }],
                intent,
                admission,
            })
        }

        ClientMouseButton::Middle => None,
        _ => None,
    }
}

fn submit_prepared_block_action(
    tick: &mut ClientTickApi<'_>,
    prepared: PreparedBlockAction,
) -> bool {
    let action_kind_id = match prepared.kind {
        PreparedBlockActionKind::Break => break_action_kind_id(),
        PreparedBlockActionKind::Place => place_action_kind_id(),
    };
    let action = prepared.action_name();

    let req = ClientActionRequest {
        action_kind_id,
        payload: Arc::from(prepared.payload),
        at_input_seq: prepared.at_input_seq,
        predicted: prepared.predicted.clone(),
    };

    match tick.client.interaction.submit_action(req) {
        Ok(action_seq) => {
            log_local_prediction_accepted(
                tick,
                action,
                action_seq,
                &prepared.intent,
                prepared.admission,
            );
            true
        }
        Err(err) => {
            log_submit_failure(tick, action, err);
            false
        }
    }
}

impl PreparedBlockAction {
    fn action_name(&self) -> &'static str {
        match self.kind {
            PreparedBlockActionKind::Break => "break",
            PreparedBlockActionKind::Place => "place",
        }
    }
}

fn coalesce_net_neutral_same_frame_place_break(
    prepared_actions: &mut Vec<PreparedBlockAction>,
    batch_predicted_edits: &mut Vec<ClientPredictedEdit>,
    next: &PreparedBlockAction,
) -> bool {
    let Some(previous) = prepared_actions.last() else {
        return false;
    };
    let Some(placed_edit) = net_neutral_place_break_predicted_edit(previous, next) else {
        return false;
    };
    if !remove_batch_predicted_edit(batch_predicted_edits, placed_edit) {
        return false;
    }

    prepared_actions.pop();
    true
}

fn net_neutral_place_break_predicted_edit(
    place: &PreparedBlockAction,
    next: &PreparedBlockAction,
) -> Option<ClientPredictedEdit> {
    if place.kind != PreparedBlockActionKind::Place || next.kind != PreparedBlockActionKind::Break {
        return None;
    }

    let [placed_edit] = place.predicted.as_slice() else {
        return None;
    };
    let [break_edit] = next.predicted.as_slice() else {
        return None;
    };

    if placed_edit.predicted_block_id.0 == 0 {
        return None;
    }
    if break_edit.predicted_block_id.0 != 0 {
        return None;
    }
    if placed_edit.pos != break_edit.pos {
        return None;
    }
    if next.admission.target_predicted_block != Some(placed_edit.predicted_block_id) {
        return None;
    }
    if next
        .admission
        .target_authoritative_block
        .is_none_or(|block| block.0 != 0)
    {
        return None;
    }

    Some(*placed_edit)
}

fn remove_batch_predicted_edit(
    batch_predicted_edits: &mut Vec<ClientPredictedEdit>,
    edit: ClientPredictedEdit,
) -> bool {
    let Some(batch_index) = batch_predicted_edits
        .iter()
        .rposition(|candidate| *candidate == edit)
    else {
        return false;
    };

    batch_predicted_edits.remove(batch_index);
    true
}

fn log_coalesced_net_neutral_same_frame_place_break(
    tick: &mut ClientTickApi<'_>,
    break_action: &PreparedBlockAction,
) {
    tracing::debug!(
        target: "freven_vanilla_essentials::client::block_interaction",
        at_input_seq = break_action.intent.identity.input_seq,
        target_pos = ?break_action.intent.hit.hit_block_pos,
        predicted_target_block = ?break_action.admission.target_predicted_block,
        authoritative_target_block = ?break_action.admission.target_authoritative_block,
        "coalesced net-neutral same-frame place-break before submit",
    );

    let message = format!(
        "coalesced net-neutral same-frame place-break before submit: at_input_seq={} \
         target_pos={:?} predicted_target_block={:?} authoritative_target_block={:?}",
        break_action.intent.identity.input_seq,
        break_action.intent.hit.hit_block_pos,
        break_action.admission.target_predicted_block,
        break_action.admission.target_authoritative_block,
    );
    tick.log(LogLevel::Debug, message.clone());
    emit_log(LogLevel::Debug, message);
}

fn log_coalesced_net_neutral_bounded_place_break(
    tick: &mut ClientTickApi<'_>,
    break_action: &PreparedBlockAction,
) {
    tracing::debug!(
        target: "freven_vanilla_essentials::client::block_interaction",
        at_input_seq = break_action.intent.identity.input_seq,
        target_pos = ?break_action.intent.hit.hit_block_pos,
        predicted_target_block = ?break_action.admission.target_predicted_block,
        authoritative_target_block = ?break_action.admission.target_authoritative_block,
        "coalesced net-neutral bounded place-break before submit",
    );

    let message = format!(
        "coalesced net-neutral bounded place-break before submit: at_input_seq={} \
         target_pos={:?} predicted_target_block={:?} authoritative_target_block={:?}",
        break_action.intent.identity.input_seq,
        break_action.intent.hit.hit_block_pos,
        break_action.admission.target_predicted_block,
        break_action.admission.target_authoritative_block,
    );
    tick.log(LogLevel::Debug, message.clone());
    emit_log(LogLevel::Debug, message);
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct LocalPredictionAdmission {
    interaction_origin_m: [f32; 3],
    target_predicted_block: Option<BlockRuntimeId>,
    target_authoritative_block: Option<BlockRuntimeId>,
    place_predicted_block: Option<BlockRuntimeId>,
    place_authoritative_block: Option<BlockRuntimeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalPredictionRejectReason {
    Terrain(TerrainInteractionRejectReasonV2),
    PlacementOccupied,
}

impl LocalPredictionRejectReason {
    const fn terrain(reason: TerrainInteractionRejectReasonV2) -> Self {
        Self::Terrain(reason)
    }
}

struct ClientPredictedTerrainWorld<'a> {
    camera: &'a dyn ClientCameraHitProvider,
    batch_predicted_edits: &'a [ClientPredictedEdit],
}

impl TerrainInteractionWorldViewV2 for ClientPredictedTerrainWorld<'_> {
    fn cell_at(&self, pos: (i32, i32, i32)) -> TerrainInteractionCellV2 {
        presented_block_id_at(self.camera, pos, self.batch_predicted_edits)
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
    batch_predicted_edits: &[ClientPredictedEdit],
) -> Option<BreakInteractionTarget> {
    let camera_ray = normalized_camera_ray(camera.camera_ray()?)?;
    let hit = raycast_presented_cursor_hit(
        camera,
        camera_ray,
        MAX_RAYCAST_DISTANCE_M,
        batch_predicted_edits,
    )?;

    Some(BreakInteractionTarget { hit, camera_ray })
}

fn select_presented_place_target(
    camera: &dyn ClientCameraHitProvider,
    batch_predicted_edits: &[ClientPredictedEdit],
) -> Option<PlaceInteractionTarget> {
    let camera_ray = normalized_camera_ray(camera.camera_ray()?)?;
    let hit = raycast_presented_cursor_hit(
        camera,
        camera_ray,
        MAX_RAYCAST_DISTANCE_M,
        batch_predicted_edits,
    )?;

    let place_pos = add_face_offset(hit.block_pos, hit.face)?;
    let place_cur = presented_block_id_at(camera, place_pos, batch_predicted_edits)?;
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
const PRESENTED_RAYCAST_GRID_TIE_EPS: f32 = 1.0e-5;

fn raycast_presented_cursor_hit(
    camera: &dyn ClientCameraHitProvider,
    camera_ray: ClientCameraRay,
    max_distance_m: f32,
    batch_predicted_edits: &[ClientPredictedEdit],
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

    if presented_block_id_at(camera, pos, batch_predicted_edits).is_some_and(|block| block.0 != 0) {
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
        let distance_m = t_max_x.min(t_max_y).min(t_max_z);

        if !distance_m.is_finite() || distance_m > max_distance_m {
            return None;
        }

        let cross_x =
            t_max_x.is_finite() && (t_max_x - distance_m).abs() <= PRESENTED_RAYCAST_GRID_TIE_EPS;
        let cross_y =
            t_max_y.is_finite() && (t_max_y - distance_m).abs() <= PRESENTED_RAYCAST_GRID_TIE_EPS;
        let cross_z =
            t_max_z.is_finite() && (t_max_z - distance_m).abs() <= PRESENTED_RAYCAST_GRID_TIE_EPS;

        hit_face = presented_raycast_entered_face_for_crossed_axes(
            dir, step_x, step_y, step_z, cross_x, cross_y, cross_z,
        )?;

        if cross_x {
            pos.0 = pos.0.checked_add(step_x)?;
            t_max_x += t_delta_x;
        }
        if cross_y {
            pos.1 = pos.1.checked_add(step_y)?;
            t_max_y += t_delta_y;
        }
        if cross_z {
            pos.2 = pos.2.checked_add(step_z)?;
            t_max_z += t_delta_z;
        }

        if presented_block_id_at(camera, pos, batch_predicted_edits)
            .is_some_and(|block| block.0 != 0)
        {
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
    batch_predicted_edits: &[ClientPredictedEdit],
) -> Option<BlockRuntimeId> {
    batch_predicted_edits
        .iter()
        .rev()
        .find(|edit| edit.pos == pos)
        .map(|edit| edit.predicted_block_id)
        .or_else(|| camera.presented_block_id_at(OWNER, pos))
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

fn presented_raycast_entered_face_for_crossed_axes(
    dir: [f32; 3],
    step_x: i32,
    step_y: i32,
    step_z: i32,
    cross_x: bool,
    cross_y: bool,
    cross_z: bool,
) -> Option<ClientBlockFace> {
    let mut best: Option<(f32, ClientBlockFace)> = None;

    if cross_x {
        best = Some((
            dir[0].abs(),
            if step_x > 0 {
                ClientBlockFace::NegX
            } else {
                ClientBlockFace::PosX
            },
        ));
    }

    if cross_y {
        let face = if step_y > 0 {
            ClientBlockFace::NegY
        } else {
            ClientBlockFace::PosY
        };
        if best.is_none_or(|(weight, _)| dir[1].abs() > weight) {
            best = Some((dir[1].abs(), face));
        }
    }

    if cross_z {
        let face = if step_z > 0 {
            ClientBlockFace::NegZ
        } else {
            ClientBlockFace::PosZ
        };
        if best.is_none_or(|(weight, _)| dir[2].abs() > weight) {
            best = Some((dir[2].abs(), face));
        }
    }

    best.map(|(_, face)| face)
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
    batch_predicted_edits: &[ClientPredictedEdit],
) -> Result<LocalPredictionAdmission, LocalPredictionRejectReason> {
    let Some(player_center_pos_m) = local_player_center_pos_m(players) else {
        return Err(LocalPredictionRejectReason::terrain(
            TerrainInteractionRejectReasonV2::PolicyDenied,
        ));
    };
    let authoritative_origin_m = humanoid_interaction_origin_m(player_center_pos_m);
    let Some(interaction_origin_m) =
        bounded_client_aim_origin_m(authoritative_origin_m, intent.ray.ray_origin_m)
    else {
        return Err(LocalPredictionRejectReason::terrain(
            TerrainInteractionRejectReasonV2::PolicyDenied,
        ));
    };
    let mut policy =
        TerrainInteractionValidationPolicyV2::new(interaction_origin_m, MAX_ACTION_REACH_M);
    policy.active_level_id = Some(intent.identity.level_id);
    policy.active_stream_epoch = Some(intent.identity.stream_epoch);
    policy.min_input_seq = Some(intent.identity.input_seq);
    policy.max_input_seq = Some(intent.identity.input_seq);

    let world = ClientPredictedTerrainWorld {
        camera,
        batch_predicted_edits,
    };
    let rules = ClientTerrainRules {
        allowed_place_block_id,
    };
    match validate_terrain_interaction_v2(&world, &rules, &policy, intent) {
        TerrainInteractionValidationV2::Accepted(_) => {}
        TerrainInteractionValidationV2::Rejected(reason) => {
            return Err(LocalPredictionRejectReason::terrain(reason));
        }
    }

    if local_place_overlaps_vanilla_humanoid_body(
        intent,
        allowed_place_block_id,
        player_center_pos_m,
    ) {
        return Err(LocalPredictionRejectReason::PlacementOccupied);
    }

    let target_pos = intent.hit.hit_block_pos;
    let target_predicted_block = presented_block_id_at(camera, target_pos, batch_predicted_edits);
    let target_authoritative_block = camera.authoritative_block_id_at(target_pos);
    let place_pos = intent.place.as_ref().map(|place| place.placement_pos);
    let place_predicted_block =
        place_pos.and_then(|pos| presented_block_id_at(camera, pos, batch_predicted_edits));
    let place_authoritative_block = place_pos.and_then(|pos| camera.authoritative_block_id_at(pos));

    Ok(LocalPredictionAdmission {
        interaction_origin_m,
        target_predicted_block,
        target_authoritative_block,
        place_predicted_block,
        place_authoritative_block,
    })
}

fn local_place_overlaps_vanilla_humanoid_body(
    intent: &TerrainInteractionIntentV2,
    allowed_place_block_id: Option<BlockRuntimeId>,
    player_center_pos_m: [f32; 3],
) -> bool {
    let Some(place) = intent.place.as_ref() else {
        return false;
    };
    if Some(place.block_id) != allowed_place_block_id {
        return false;
    }

    // rc10 Vanilla places one authored full-collision cube. The long-term
    // generic client-side body/shape query is tracked in freven-sdk#127.
    let mut occupied = false;
    visit_vanilla_client_place_collision_boxes(
        place.block_id,
        allowed_place_block_id,
        &mut |bounds| {
            if block_collision_box_overlaps_aabb(
                place.placement_pos,
                bounds,
                player_center_pos_m,
                humanoid_collision_half_extents_m(),
            ) {
                occupied = true;
            }
        },
    );
    occupied
}

fn visit_vanilla_client_place_collision_boxes(
    block_id: BlockRuntimeId,
    allowed_place_block_id: Option<BlockRuntimeId>,
    emit: &mut dyn FnMut(BlockShapeBox),
) {
    if Some(block_id) == allowed_place_block_id {
        emit(BlockShapeBox::full_block());
    }
}

fn axis_ranges_overlap_strict(a_min: f32, a_max: f32, b_min: f32, b_max: f32) -> bool {
    a_min < b_max && a_max > b_min
}

fn block_collision_box_overlaps_aabb(
    block_pos: (i32, i32, i32),
    bounds: BlockShapeBox,
    center_m: [f32; 3],
    half_extents_m: [f32; 3],
) -> bool {
    let block_min = [
        block_pos.0 as f32 + bounds.min[0],
        block_pos.1 as f32 + bounds.min[1],
        block_pos.2 as f32 + bounds.min[2],
    ];
    let block_max = [
        block_pos.0 as f32 + bounds.max[0],
        block_pos.1 as f32 + bounds.max[1],
        block_pos.2 as f32 + bounds.max[2],
    ];
    let body_min = [
        center_m[0] - half_extents_m[0],
        center_m[1] - half_extents_m[1],
        center_m[2] - half_extents_m[2],
    ];
    let body_max = [
        center_m[0] + half_extents_m[0],
        center_m[1] + half_extents_m[1],
        center_m[2] + half_extents_m[2],
    ];

    axis_ranges_overlap_strict(block_min[0], block_max[0], body_min[0], body_max[0])
        && axis_ranges_overlap_strict(block_min[1], block_max[1], body_min[1], body_max[1])
        && axis_ranges_overlap_strict(block_min[2], block_max[2], body_min[2], body_max[2])
}

fn local_player_center_pos_m(players: &mut dyn ClientPlayerProvider) -> Option<[f32; 3]> {
    let mut views = Vec::<ClientPlayerView>::new();
    players.list_players(&mut views);
    views
        .into_iter()
        .find(|view| view.is_local)
        .map(|view| [view.world_pos_m.0, view.world_pos_m.1, view.world_pos_m.2])
}

fn log_local_skip(tick: &mut ClientTickApi<'_>, action: ClientMouseButton, reason: &str) {
    tracing::debug!(
        target: "freven_vanilla_essentials::client::block_interaction",
        action = action_name(action),
        reason,
        "block interaction not submitted",
    );
    let message = format!(
        "{} interaction not submitted: {reason}",
        action_name(action)
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
    reason: LocalPredictionRejectReason,
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
        "block interaction local prediction rejected",
    );
    tick.log(
        LogLevel::Debug,
        format!(
            "{action} interaction not submitted: local v2 prediction validation rejected \
             reason={reason:?} action_seq=missing-pre-submit at_input_seq={} target_pos={:?} \
             place_pos={:?} hit_face={:?} ray_origin_m={:?} ray_dir={:?} prediction_tx={:?} \
             depends_on={:?}",
            intent.identity.input_seq,
            intent.hit.hit_block_pos,
            intent.place.as_ref().map(|place| place.placement_pos),
            intent.hit.hit_face,
            intent.ray.ray_origin_m,
            intent.ray.ray_dir,
            intent.identity.prediction_tx,
            intent.identity.depends_on,
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
        "block interaction local prediction accepted",
    );

    let message = format!(
        "{action} local prediction accepted: action_seq={action_seq} at_input_seq={} \
         target_pos={:?} place_pos={:?} hit_face={:?} ray_origin_m={:?} ray_dir={:?} \
         client_interaction_origin_m={:?} predicted_target_block={:?} \
         authoritative_target_block={:?} predicted_place_block={:?} \
         authoritative_place_block={:?} prediction_tx={:?} depends_on={:?}",
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

        fn presented_block_id_at(
            &self,
            _owner: &str,
            pos: (i32, i32, i32),
        ) -> Option<BlockRuntimeId> {
            self.predicted_block_id_at(pos)
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

        fn visit_collision_boxes(
            &self,
            block_id: BlockRuntimeId,
            emit: &mut dyn FnMut(freven_block_sdk_types::BlockShapeBox),
        ) {
            if self.is_solid(block_id) {
                emit(freven_block_sdk_types::BlockShapeBox::full_block());
            }
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

        fn player_collision_aabb(
            &self,
            _player_id: u64,
        ) -> Option<freven_world_api::PlayerCollisionAabb> {
            Some(freven_world_api::PlayerCollisionAabb::new(
                self.pos,
                [0.3, 0.9, 0.3],
            ))
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
    fn engine_presented_predicted_only_break_target_is_locally_submitted() {
        ensure_action_kinds();

        // Vanilla trusts engine-presented terrain for local prediction.
        // Stale overlay lifecycle is owned by the engine prediction/reconciliation layer.
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
            let mut tick = ClientTickApi::new(11, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert_eq!(interaction.requests.len(), 1);
        assert_eq!(
            interaction.requests[0].action_kind_id,
            break_action_kind_id()
        );
        assert_eq!(
            interaction.requests[0].predicted,
            vec![ClientPredictedEdit::clear_block((2, 1, 0))]
        );
    }

    #[test]
    fn engine_presented_empty_place_cell_is_locally_submitted() {
        ensure_action_kinds();

        // Vanilla trusts engine-presented terrain for local prediction.
        // Stale overlay lifecycle is owned by the engine prediction/reconciliation layer.
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
        .with_block((2, 1, 0), 1)
        .with_predicted_block((1, 1, 0), 0)
        .with_authoritative_block((1, 1, 0), 1);
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
            let mut tick = ClientTickApi::new(12, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert_eq!(interaction.requests.len(), 1);
        assert_eq!(
            interaction.requests[0].action_kind_id,
            place_action_kind_id()
        );
        assert_eq!(
            interaction.requests[0].predicted,
            vec![ClientPredictedEdit {
                pos: (1, 1, 0),
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
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
    fn ordered_same_tick_double_right_submits_two_place_actions() {
        ensure_action_kinds();

        let mut services = NoopServices;
        let mut input =
            OrderedTestInput::new(vec![ClientMouseButton::Right, ClientMouseButton::Right]);
        let mut camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (3, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((1, 1, 0), 0)
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
                pos: (2, 1, 0),
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
        assert_eq!(
            interaction.requests[1].predicted,
            vec![ClientPredictedEdit {
                pos: (1, 1, 0),
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
    }

    #[test]
    fn isolated_place_intent_flushes_after_bounded_cancel_window() {
        ensure_action_kinds();

        let mut state = BlockInteractionClientState::default();
        let mut services = NoopServices;
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
            let mut input = OrderedTestInput::new(vec![ClientMouseButton::Right]);
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut camera,
                &mut interaction,
                &mut players,
            );
            let mut tick = ClientTickApi::new(23, std::time::Duration::from_millis(33), client);
            state.on_tick_client(&mut tick);
        }

        assert!(
            interaction.requests.is_empty(),
            "isolated place should be held for one bounded cancel window before submit"
        );
        assert!(
            state.pending_place.is_some(),
            "held place intent must live in runtime-owned client lifecycle state"
        );

        {
            let mut input = OrderedTestInput::new(Vec::<ClientMouseButton>::new());
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut camera,
                &mut interaction,
                &mut players,
            );
            let mut tick = ClientTickApi::new(24, std::time::Duration::from_millis(33), client);
            state.on_tick_client(&mut tick);
        }

        assert_eq!(
            interaction.requests.len(),
            1,
            "place must flush after the bounded cancel window when no inverse arrives"
        );
        assert_eq!(
            interaction.requests[0].action_kind_id,
            place_action_kind_id()
        );
        assert!(
            state.pending_place.is_none(),
            "pending place must be consumed after flush"
        );
    }

    #[test]
    fn near_frame_place_then_break_predicted_only_block_coalesces_to_noop() {
        ensure_action_kinds();

        let mut state = BlockInteractionClientState::default();
        let mut services = NoopServices;
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
            let mut input = OrderedTestInput::new(vec![ClientMouseButton::Right]);
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut camera,
                &mut interaction,
                &mut players,
            );
            let mut tick = ClientTickApi::new(23, std::time::Duration::from_millis(33), client);
            state.on_tick_client(&mut tick);
        }

        assert!(
            interaction.requests.is_empty(),
            "first isolated place must not submit before the cancel window closes"
        );

        {
            let mut input = OrderedTestInput::new(vec![ClientMouseButton::Left]);
            let client = ClientApi::new(
                &mut services,
                &mut input,
                &mut camera,
                &mut interaction,
                &mut players,
            );
            let mut tick = ClientTickApi::new(24, std::time::Duration::from_millis(33), client);
            state.on_tick_client(&mut tick);
        }

        assert!(
            interaction.requests.is_empty(),
            "near-frame break of the exact pending predicted-only placed block is net-neutral and must not submit server actions"
        );
        assert!(
            state.pending_place.is_none(),
            "pending place must be consumed by exact inverse coalescing"
        );
    }

    #[test]
    fn presented_raycast_edge_tie_steps_all_crossed_axes_before_testing_hit() {
        let camera_ray = ClientCameraRay {
            origin: [0.5, 1.5, -0.5],
            direction: [0.0, -0.70710677, 0.70710677],
        };

        let mut camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (0, 0, 0),
            face: ClientBlockFace::PosY,
            distance_m: 1.0,
        })
        .with_block((0, 0, -1), 1)
        .with_block((0, 0, 0), 2);
        camera.camera_ray = Some(camera_ray);

        let hit = raycast_presented_cursor_hit(&camera, camera_ray, 10.0, &[])
            .expect("ray should hit the geometrically entered block");

        assert_eq!(
            hit.block_pos,
            (0, 0, 0),
            "presented interaction raycast must step all tied grid axes before testing occupancy"
        );
    }

    #[test]
    fn presented_raycast_corner_tie_does_not_pick_single_axis_side_voxel() {
        let camera_ray = ClientCameraRay {
            origin: [0.5, 0.5, 0.5],
            direction: [0.57735026, 0.57735026, 0.57735026],
        };

        let mut camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (1, 1, 1),
            face: ClientBlockFace::NegX,
            distance_m: 1.0,
        })
        .with_block((1, 0, 0), 1)
        .with_block((0, 1, 0), 1)
        .with_block((0, 0, 1), 1)
        .with_block((1, 1, 1), 2);
        camera.camera_ray = Some(camera_ray);

        let hit = raycast_presented_cursor_hit(&camera, camera_ray, 10.0, &[])
            .expect("ray should hit the corner-entered block");

        assert_eq!(
            hit.block_pos,
            (1, 1, 1),
            "presented interaction corner ties must not select a side-touch voxel"
        );
    }

    #[test]
    fn same_tick_place_then_break_predicted_only_block_coalesces_to_noop() {
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

        assert!(
            interaction.requests.is_empty(),
            "same-drain place followed by break of the exact predicted-only placed block is net-neutral and must not submit server actions"
        );
    }

    #[test]
    fn same_tick_left_then_right_is_not_net_neutral_place_break_coalesce() {
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
            let mut tick = ClientTickApi::new(24, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        assert_eq!(
            interaction.requests.len(),
            2,
            "break->place is a real ordered terrain change and must not be collapsed by the place->break no-op rule"
        );
        assert_eq!(
            interaction.requests[0].action_kind_id,
            break_action_kind_id()
        );
        assert_eq!(
            interaction.requests[1].action_kind_id,
            place_action_kind_id()
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
            block_pos: (3, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((2, 1, 0), 0)
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
            let mut tick = ClientTickApi::new(17, std::time::Duration::from_millis(33), client);
            tick_client(&mut tick);
        }

        let mut input = TestInput {
            left: false,
            right: true,
        };
        let mut second_camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (2, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 0.5,
        })
        .with_block((1, 1, 0), 0)
        .with_predicted_block((2, 1, 0), 3)
        .with_authoritative_block((2, 1, 0), 0)
        .with_block((3, 1, 0), 1)
        .with_authoritative_hit(Some(ClientCursorHit {
            block_pos: (3, 1, 0),
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
                pos: (2, 1, 0),
                predicted_block_id: BlockRuntimeId(3),
            }]
        );
        assert_eq!(
            interaction.requests[1].predicted,
            vec![ClientPredictedEdit {
                pos: (1, 1, 0),
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
            validate_local_prediction_admission(&camera, &mut players, None, &intent, &[]);

        assert!(
            client_validation.is_ok(),
            "#79-style predicted-vs-authoritative block-id mismatch alone must not reject a \
             presented-valid local prediction: {client_validation:?}"
        );
    }

    #[test]
    fn local_place_inside_player_body_rejects_before_prediction_submit() {
        ensure_action_kinds();

        let intent = server_place_intent((1, 1, 0), (0, 1, 0), BlockRuntimeId(3), 42, 1);
        let camera = PresentedCamera::new(ClientCursorHit {
            block_pos: (1, 1, 0),
            face: ClientBlockFace::NegX,
            distance_m: 1.5,
        })
        .with_block((1, 1, 0), 1)
        .with_block((0, 1, 0), 0);
        let mut players = NoopPlayers;

        let client_validation = validate_local_prediction_admission(
            &camera,
            &mut players,
            Some(BlockRuntimeId(3)),
            &intent,
            &[],
        );

        assert_eq!(
            client_validation,
            Err(LocalPredictionRejectReason::PlacementOccupied)
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
            validate_local_prediction_admission(&camera, &mut players, None, &intent, &[]);

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
            validate_local_prediction_admission(&camera, &mut players, None, &intent, &[]);

        assert_eq!(
            client_validation,
            Err(LocalPredictionRejectReason::terrain(
                TerrainInteractionRejectReasonV2::Occluded
            ))
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
    fn server_rejects_v2_place_inside_player_body_with_placement_occupied_reason() {
        let physics = TestPhysics::default();
        let payload = try_encode_place_payload_v2(&server_place_intent(
            (1, 1, 0),
            (0, 1, 0),
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
            .with_block((0, 1, 0), 0)
            .with_block((1, 1, 0), 1);
        let mut ctx = ActionContext::new(
            None,
            Some(&mut authority),
            Some(&physics),
            Some(&mut services),
            7,
            42,
        );

        let mut handler = crate::actions::place::PlaceActionHandler;
        assert_eq!(
            handler.handle(&mut ctx, &cmd),
            ActionOutcome::RejectedWithReason(
                freven_world_api::ActionRejectReason::PlacementOccupied
            )
        );
        assert_eq!(authority.block(0, 1, 0), Some(BlockRuntimeId(0)));
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
    fn bounded_client_aim_origin_preserves_presented_hit_when_body_origin_is_occluded() {
        let mut intent = server_break_intent((16, 7, 19), 2.1, 721, 16);
        intent.ray.ray_origin_m = [16.264812, 7.6210003, 19.3];
        intent.ray.ray_dir = [0.06249107, -0.7858575, -0.6152422];
        intent.hit.hit_face = ClientBlockFace::PosY;

        let authority = TestAuthority::default()
            .with_block((16, 7, 19), 21)
            .with_block((15, 5, 18), 13);
        let world = BlockWorldViewTerrainAdapter::new(&authority);
        let rules = TestTerrainRules { world: &authority };

        let authoritative_body_origin = [15.7, 7.6210003, 19.3];
        let body_policy = TerrainInteractionValidationPolicyV2::new(
            authoritative_body_origin,
            MAX_ACTION_REACH_M,
        );
        assert_eq!(
            validate_terrain_interaction_v2(&world, &rules, &body_policy, &intent),
            TerrainInteractionValidationV2::Rejected(TerrainInteractionRejectReasonV2::Occluded),
            "the #400 body-origin path hits the intervening occluder"
        );

        let bounded_origin =
            bounded_client_aim_origin_m(authoritative_body_origin, intent.ray.ray_origin_m)
                .expect("normal camera lateral offset must be within Vanilla humanoid aim volume");
        let bounded_policy =
            TerrainInteractionValidationPolicyV2::new(bounded_origin, MAX_ACTION_REACH_M);

        assert!(
            matches!(
                validate_terrain_interaction_v2(&world, &rules, &bounded_policy, &intent),
                TerrainInteractionValidationV2::Accepted(_)
            ),
            "server validation should use the bounded captured camera/aim origin, not a different body-origin ray"
        );
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
