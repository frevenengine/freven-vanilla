use std::sync::Arc;

use crate::{break_action_kind_id, place_action_kind_id};
use freven_api::action_payloads::{ActionTarget, encode_break_payload_v1, encode_place_payload_v1};
use freven_api::{ClientActionRequest, ClientBlockFace, ClientMouseButton, ClientPredictedEdit};
use freven_core::blocks::storage;

const OWNER: &str = "freven.vanilla:block_interaction";
const MAX_RAYCAST_DISTANCE_M: f32 = 5.0;
const BREAK_STATUS_FINISHED: u8 = 2;
const PLACE_BLOCK_ID: u8 = storage::STONE;

pub fn start_client(api: &mut freven_api::ClientApi<'_>) {
    let _ = api.input.bind_mouse_button(ClientMouseButton::Left, OWNER);
    let _ = api.input.bind_mouse_button(ClientMouseButton::Right, OWNER);
}

pub fn tick_client(tick: &mut freven_api::ClientTickApi<'_>) {
    let api = &mut tick.client;

    // Consume one click per tick (owner-guarded).
    let action = if api
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
    };

    let Some(action) = action else {
        return;
    };

    // We only allow submitting actions when the client has an active stream.
    if api.interaction.active_stream().is_none() {
        return;
    }

    let Some(hit) = api.camera.cursor_hit(MAX_RAYCAST_DISTANCE_M) else {
        return;
    };

    let at_input_seq = api.interaction.next_input_seq();
    let target = ActionTarget {
        pos: hit.block_pos,
        face: client_face_to_wire(hit.face),
    };

    match action {
        ClientMouseButton::Left => {
            // Break: require non-air at the hit position.
            let Some(current) = api.camera.block_id_at(hit.block_pos) else {
                return;
            };
            if storage::is_air(current) {
                return;
            }

            let payload = encode_break_payload_v1(BREAK_STATUS_FINISHED, target);

            let req = ClientActionRequest {
                action_kind_id: break_action_kind_id(),
                payload: Arc::from(payload),
                at_input_seq,
                predicted: vec![ClientPredictedEdit {
                    pos: hit.block_pos,
                    predicted_block_id: storage::AIR,
                }],
            };

            // Engine assigns action_seq and owns retransmit/prediction.
            let _ = api.interaction.submit_action(req);
        }

        ClientMouseButton::Right => {
            // Place: compute adjacent placement cell and require air.
            let Some(place_pos) = add_face_offset(hit.block_pos, hit.face) else {
                return;
            };
            let Some(current) = api.camera.block_id_at(place_pos) else {
                return;
            };
            if !storage::is_air(current) {
                return;
            }

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

            let _ = api.interaction.submit_action(req);
        }

        ClientMouseButton::Middle => {}
    }
}

fn client_face_to_wire(face: ClientBlockFace) -> u8 {
    match face {
        ClientBlockFace::NegX => 0,
        ClientBlockFace::PosX => 1,
        ClientBlockFace::NegY => 2,
        ClientBlockFace::PosY => 3,
        ClientBlockFace::NegZ => 4,
        ClientBlockFace::PosZ => 5,
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
    }
}
