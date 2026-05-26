use freven_block_api::{
    ClientBlockFace, TerrainInteractionHitV2, TerrainInteractionIdentityV2,
    TerrainInteractionIntentV2, TerrainInteractionKindV2, TerrainInteractionRayV2,
    TerrainPlaceIntentV2, TerrainPredictionTransactionIdV2,
};
use freven_block_sdk_types::BlockRuntimeId;

const TERRAIN_INTERACTION_V2_MAGIC: &[u8; 4] = b"FVT2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionTarget {
    pub pos: (i32, i32, i32),
    pub face: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakPayloadV1 {
    pub status: u8,
    pub target: ActionTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacePayloadV1 {
    pub target: ActionTarget,
    pub block_id: u8,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ActionPayloadError {
    #[error("payload ended early while reading {field}")]
    UnexpectedEof { field: &'static str },
    #[error("varint for {field} exceeds u32 encoding bounds")]
    VarintTooLong { field: &'static str },
    #[error("varint for {field} exceeds u64 encoding bounds")]
    VarintU64TooLong { field: &'static str },
    #[error("payload has an invalid v2 header")]
    InvalidV2Header,
    #[error("payload has an invalid v2 enum value for {field}: {value}")]
    InvalidEnumValue { field: &'static str, value: u8 },
    #[error("payload has an unsupported client block face")]
    UnsupportedClientBlockFace,
    #[error("payload dependency count does not fit this runtime")]
    DependencyCountTooLarge,
    #[error("payload contains trailing bytes")]
    TrailingBytes,
}

#[must_use]
pub fn encode_break_payload_v1(status: u8, target: ActionTarget) -> Vec<u8> {
    let mut out = Vec::with_capacity(17);
    out.push(status);
    write_var_i32(&mut out, target.pos.0);
    write_var_i32(&mut out, target.pos.1);
    write_var_i32(&mut out, target.pos.2);
    out.push(target.face);
    out
}

pub fn decode_break_payload_v1(payload: &[u8]) -> Result<BreakPayloadV1, ActionPayloadError> {
    let mut cursor = 0usize;
    let status = read_u8(payload, &mut cursor, "status")?;
    let pos_x = read_var_i32(payload, &mut cursor, "pos_x")?;
    let pos_y = read_var_i32(payload, &mut cursor, "pos_y")?;
    let pos_z = read_var_i32(payload, &mut cursor, "pos_z")?;
    let face = read_u8(payload, &mut cursor, "face")?;
    if cursor != payload.len() {
        return Err(ActionPayloadError::TrailingBytes);
    }
    Ok(BreakPayloadV1 {
        status,
        target: ActionTarget {
            pos: (pos_x, pos_y, pos_z),
            face,
        },
    })
}

#[must_use]
pub fn encode_place_payload_v1(target: ActionTarget, block_id: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(17);
    write_var_i32(&mut out, target.pos.0);
    write_var_i32(&mut out, target.pos.1);
    write_var_i32(&mut out, target.pos.2);
    out.push(target.face);
    out.push(block_id);
    out
}

pub fn decode_place_payload_v1(payload: &[u8]) -> Result<PlacePayloadV1, ActionPayloadError> {
    let mut cursor = 0usize;
    let hit_x = read_var_i32(payload, &mut cursor, "hit_pos_x")?;
    let hit_y = read_var_i32(payload, &mut cursor, "hit_pos_y")?;
    let hit_z = read_var_i32(payload, &mut cursor, "hit_pos_z")?;
    let face = read_u8(payload, &mut cursor, "face")?;
    let block_id = read_u8(payload, &mut cursor, "block_id")?;
    if cursor != payload.len() {
        return Err(ActionPayloadError::TrailingBytes);
    }
    Ok(PlacePayloadV1 {
        target: ActionTarget {
            pos: (hit_x, hit_y, hit_z),
            face,
        },
        block_id,
    })
}

pub fn try_encode_break_payload_v2(
    intent: &TerrainInteractionIntentV2,
) -> Result<Vec<u8>, ActionPayloadError> {
    debug_assert_eq!(intent.identity.kind, TerrainInteractionKindV2::Break);
    debug_assert!(intent.place.is_none());
    try_encode_terrain_interaction_payload_v2(intent)
}

pub fn decode_break_payload_v2(
    payload: &[u8],
) -> Result<TerrainInteractionIntentV2, ActionPayloadError> {
    let intent = decode_terrain_interaction_payload_v2(payload)?;
    if intent.identity.kind != TerrainInteractionKindV2::Break || intent.place.is_some() {
        return Err(ActionPayloadError::InvalidEnumValue {
            field: "kind",
            value: kind_to_wire(intent.identity.kind),
        });
    }
    Ok(intent)
}

pub fn try_encode_place_payload_v2(
    intent: &TerrainInteractionIntentV2,
) -> Result<Vec<u8>, ActionPayloadError> {
    debug_assert_eq!(intent.identity.kind, TerrainInteractionKindV2::Place);
    debug_assert!(intent.place.is_some());
    try_encode_terrain_interaction_payload_v2(intent)
}

pub fn decode_place_payload_v2(
    payload: &[u8],
) -> Result<TerrainInteractionIntentV2, ActionPayloadError> {
    let intent = decode_terrain_interaction_payload_v2(payload)?;
    if intent.identity.kind != TerrainInteractionKindV2::Place || intent.place.is_none() {
        return Err(ActionPayloadError::InvalidEnumValue {
            field: "kind",
            value: kind_to_wire(intent.identity.kind),
        });
    }
    Ok(intent)
}

pub fn try_encode_terrain_interaction_payload_v2(
    intent: &TerrainInteractionIntentV2,
) -> Result<Vec<u8>, ActionPayloadError> {
    let mut out = Vec::with_capacity(96);
    out.extend_from_slice(TERRAIN_INTERACTION_V2_MAGIC);

    out.push(kind_to_wire(intent.identity.kind));
    write_var_u32(&mut out, intent.identity.level_id);
    write_var_u32(&mut out, intent.identity.stream_epoch);
    write_var_u32(&mut out, intent.identity.input_seq);
    write_optional_var_u32(&mut out, intent.identity.action_seq);
    write_var_u64(&mut out, intent.identity.prediction_tx.0);
    let dependency_count = u32::try_from(intent.identity.depends_on.len())
        .map_err(|_| ActionPayloadError::DependencyCountTooLarge)?;
    write_var_u32(&mut out, dependency_count);
    for dependency in &intent.identity.depends_on {
        write_var_u64(&mut out, dependency.0);
    }

    write_vec3_f32(&mut out, intent.ray.ray_origin_m);
    write_vec3_f32(&mut out, intent.ray.ray_dir);
    write_f32(&mut out, intent.ray.max_distance_m);
    write_optional_var_u64(&mut out, intent.ray.client_view_tick);

    write_pos(&mut out, intent.hit.hit_block_pos);
    out.push(face_to_wire(intent.hit.hit_face)?);

    match intent.place {
        Some(place) => {
            out.push(1);
            write_pos(&mut out, place.support_block_pos);
            write_pos(&mut out, place.placement_pos);
            write_var_u32(&mut out, place.block_id.0);
            out.push(u8::from(place.expected_placement_empty));
            out.push(u8::from(place.expected_support_solid));
        }
        None => out.push(0),
    }

    Ok(out)
}

pub fn decode_terrain_interaction_payload_v2(
    payload: &[u8],
) -> Result<TerrainInteractionIntentV2, ActionPayloadError> {
    let mut cursor = 0usize;
    if payload.len() < TERRAIN_INTERACTION_V2_MAGIC.len()
        || &payload[..TERRAIN_INTERACTION_V2_MAGIC.len()] != TERRAIN_INTERACTION_V2_MAGIC
    {
        return Err(ActionPayloadError::InvalidV2Header);
    }
    cursor += TERRAIN_INTERACTION_V2_MAGIC.len();

    let kind = read_kind(payload, &mut cursor)?;
    let level_id = read_var_u32(payload, &mut cursor, "level_id")?;
    let stream_epoch = read_var_u32(payload, &mut cursor, "stream_epoch")?;
    let input_seq = read_var_u32(payload, &mut cursor, "input_seq")?;
    let action_seq = read_optional_var_u32(payload, &mut cursor, "action_seq")?;
    let prediction_tx =
        TerrainPredictionTransactionIdV2(read_var_u64(payload, &mut cursor, "prediction_tx")?);
    let dependency_count = read_var_u32(payload, &mut cursor, "dependency_count")?;
    let dependency_count = usize::try_from(dependency_count)
        .map_err(|_| ActionPayloadError::DependencyCountTooLarge)?;
    let mut depends_on = Vec::with_capacity(dependency_count);
    for _ in 0..dependency_count {
        depends_on.push(TerrainPredictionTransactionIdV2(read_var_u64(
            payload,
            &mut cursor,
            "dependency",
        )?));
    }

    let ray = TerrainInteractionRayV2 {
        ray_origin_m: read_vec3_f32(payload, &mut cursor, "ray_origin_m")?,
        ray_dir: read_vec3_f32(payload, &mut cursor, "ray_dir")?,
        max_distance_m: read_f32(payload, &mut cursor, "max_distance_m")?,
        client_view_tick: read_optional_var_u64(payload, &mut cursor, "client_view_tick")?,
    };
    let hit = TerrainInteractionHitV2 {
        hit_block_pos: read_pos(payload, &mut cursor, "hit_block_pos")?,
        hit_face: read_face(payload, &mut cursor)?,
        hit_point_m: None,
    };

    let place = match read_u8(payload, &mut cursor, "has_place")? {
        0 => None,
        1 => Some(TerrainPlaceIntentV2 {
            support_block_pos: read_pos(payload, &mut cursor, "support_block_pos")?,
            placement_pos: read_pos(payload, &mut cursor, "placement_pos")?,
            block_id: BlockRuntimeId(read_var_u32(payload, &mut cursor, "block_id")?),
            expected_placement_empty: read_bool(payload, &mut cursor, "expected_placement_empty")?,
            expected_support_solid: read_bool(payload, &mut cursor, "expected_support_solid")?,
        }),
        value => {
            return Err(ActionPayloadError::InvalidEnumValue {
                field: "has_place",
                value,
            });
        }
    };

    if cursor != payload.len() {
        return Err(ActionPayloadError::TrailingBytes);
    }

    Ok(TerrainInteractionIntentV2 {
        identity: TerrainInteractionIdentityV2 {
            level_id,
            stream_epoch,
            input_seq,
            action_seq,
            kind,
            prediction_tx,
            depends_on,
        },
        ray,
        hit,
        place,
    })
}

#[inline]
fn write_var_u32(out: &mut Vec<u8>, mut value: u32) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7F) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

#[inline]
fn write_var_i32(out: &mut Vec<u8>, value: i32) {
    let zigzag = ((value << 1) ^ (value >> 31)) as u32;
    write_var_u32(out, zigzag);
}

#[inline]
fn write_var_u64(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7F) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

#[inline]
fn write_optional_var_u32(out: &mut Vec<u8>, value: Option<u32>) {
    match value {
        Some(value) => {
            out.push(1);
            write_var_u32(out, value);
        }
        None => out.push(0),
    }
}

#[inline]
fn write_optional_var_u64(out: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => {
            out.push(1);
            write_var_u64(out, value);
        }
        None => out.push(0),
    }
}

#[inline]
fn write_f32(out: &mut Vec<u8>, value: f32) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[inline]
fn write_vec3_f32(out: &mut Vec<u8>, value: [f32; 3]) {
    for axis in value {
        write_f32(out, axis);
    }
}

#[inline]
fn write_pos(out: &mut Vec<u8>, pos: (i32, i32, i32)) {
    write_var_i32(out, pos.0);
    write_var_i32(out, pos.1);
    write_var_i32(out, pos.2);
}

fn read_u8(
    payload: &[u8],
    cursor: &mut usize,
    field: &'static str,
) -> Result<u8, ActionPayloadError> {
    let Some(byte) = payload.get(*cursor).copied() else {
        return Err(ActionPayloadError::UnexpectedEof { field });
    };
    *cursor += 1;
    Ok(byte)
}

fn read_bool(
    payload: &[u8],
    cursor: &mut usize,
    field: &'static str,
) -> Result<bool, ActionPayloadError> {
    match read_u8(payload, cursor, field)? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(ActionPayloadError::InvalidEnumValue { field, value }),
    }
}

fn read_var_u32(
    payload: &[u8],
    cursor: &mut usize,
    field: &'static str,
) -> Result<u32, ActionPayloadError> {
    let mut result = 0u32;
    let mut shift = 0u32;
    for i in 0..5 {
        let byte = read_u8(payload, cursor, field)?;
        if i == 4 && byte > 0x0F {
            return Err(ActionPayloadError::VarintTooLong { field });
        }
        result |= u32::from(byte & 0x7F) << shift;
        if (byte & 0x80) == 0 {
            return Ok(result);
        }
        shift += 7;
    }
    Err(ActionPayloadError::VarintTooLong { field })
}

fn read_var_u64(
    payload: &[u8],
    cursor: &mut usize,
    field: &'static str,
) -> Result<u64, ActionPayloadError> {
    let mut result = 0u64;
    let mut shift = 0u32;
    for i in 0..10 {
        let byte = read_u8(payload, cursor, field)?;
        if i == 9 && byte > 0x01 {
            return Err(ActionPayloadError::VarintU64TooLong { field });
        }
        result |= u64::from(byte & 0x7F) << shift;
        if (byte & 0x80) == 0 {
            return Ok(result);
        }
        shift += 7;
    }
    Err(ActionPayloadError::VarintU64TooLong { field })
}

#[inline]
fn read_var_i32(
    payload: &[u8],
    cursor: &mut usize,
    field: &'static str,
) -> Result<i32, ActionPayloadError> {
    let raw = read_var_u32(payload, cursor, field)?;
    Ok(((raw >> 1) as i32) ^ (-((raw & 1) as i32)))
}

fn read_optional_var_u32(
    payload: &[u8],
    cursor: &mut usize,
    field: &'static str,
) -> Result<Option<u32>, ActionPayloadError> {
    match read_u8(payload, cursor, field)? {
        0 => Ok(None),
        1 => Ok(Some(read_var_u32(payload, cursor, field)?)),
        value => Err(ActionPayloadError::InvalidEnumValue { field, value }),
    }
}

fn read_optional_var_u64(
    payload: &[u8],
    cursor: &mut usize,
    field: &'static str,
) -> Result<Option<u64>, ActionPayloadError> {
    match read_u8(payload, cursor, field)? {
        0 => Ok(None),
        1 => Ok(Some(read_var_u64(payload, cursor, field)?)),
        value => Err(ActionPayloadError::InvalidEnumValue { field, value }),
    }
}

fn read_f32(
    payload: &[u8],
    cursor: &mut usize,
    field: &'static str,
) -> Result<f32, ActionPayloadError> {
    let end = cursor
        .checked_add(4)
        .ok_or(ActionPayloadError::UnexpectedEof { field })?;
    let Some(bytes) = payload.get(*cursor..end) else {
        return Err(ActionPayloadError::UnexpectedEof { field });
    };
    *cursor = end;
    Ok(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_vec3_f32(
    payload: &[u8],
    cursor: &mut usize,
    field: &'static str,
) -> Result<[f32; 3], ActionPayloadError> {
    Ok([
        read_f32(payload, cursor, field)?,
        read_f32(payload, cursor, field)?,
        read_f32(payload, cursor, field)?,
    ])
}

fn read_pos(
    payload: &[u8],
    cursor: &mut usize,
    field: &'static str,
) -> Result<(i32, i32, i32), ActionPayloadError> {
    Ok((
        read_var_i32(payload, cursor, field)?,
        read_var_i32(payload, cursor, field)?,
        read_var_i32(payload, cursor, field)?,
    ))
}

fn kind_to_wire(kind: TerrainInteractionKindV2) -> u8 {
    match kind {
        TerrainInteractionKindV2::Break => 0,
        TerrainInteractionKindV2::Place => 1,
    }
}

fn read_kind(
    payload: &[u8],
    cursor: &mut usize,
) -> Result<TerrainInteractionKindV2, ActionPayloadError> {
    match read_u8(payload, cursor, "kind")? {
        0 => Ok(TerrainInteractionKindV2::Break),
        1 => Ok(TerrainInteractionKindV2::Place),
        value => Err(ActionPayloadError::InvalidEnumValue {
            field: "kind",
            value,
        }),
    }
}

fn face_to_wire(face: ClientBlockFace) -> Result<u8, ActionPayloadError> {
    match face {
        ClientBlockFace::NegX => Ok(0),
        ClientBlockFace::PosX => Ok(1),
        ClientBlockFace::NegY => Ok(2),
        ClientBlockFace::PosY => Ok(3),
        ClientBlockFace::NegZ => Ok(4),
        ClientBlockFace::PosZ => Ok(5),
        _ => Err(ActionPayloadError::UnsupportedClientBlockFace),
    }
}

fn read_face(payload: &[u8], cursor: &mut usize) -> Result<ClientBlockFace, ActionPayloadError> {
    match read_u8(payload, cursor, "hit_face")? {
        0 => Ok(ClientBlockFace::NegX),
        1 => Ok(ClientBlockFace::PosX),
        2 => Ok(ClientBlockFace::NegY),
        3 => Ok(ClientBlockFace::PosY),
        4 => Ok(ClientBlockFace::NegZ),
        5 => Ok(ClientBlockFace::PosZ),
        value => Err(ActionPayloadError::InvalidEnumValue {
            field: "hit_face",
            value,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACTION_PAYLOAD_LIMIT_BYTES: usize = 64;
    const COMPACT_BREAK_V2_BYTES: usize = 52;
    const COMPACT_PLACE_V2_BYTES: usize = 63;

    #[test]
    fn roundtrip_break_payload_v1() {
        let payload = encode_break_payload_v1(
            2,
            ActionTarget {
                pos: (10, 64, -5),
                face: 3,
            },
        );
        let decoded = decode_break_payload_v1(&payload).expect("decode break");
        assert_eq!(
            decoded,
            BreakPayloadV1 {
                status: 2,
                target: ActionTarget {
                    pos: (10, 64, -5),
                    face: 3
                }
            }
        );
    }

    #[test]
    fn roundtrip_place_payload_v1() {
        let payload = encode_place_payload_v1(
            ActionTarget {
                pos: (10, 64, -5),
                face: 3,
            },
            1,
        );
        let decoded = decode_place_payload_v1(&payload).expect("decode place");
        assert_eq!(
            decoded,
            PlacePayloadV1 {
                target: ActionTarget {
                    pos: (10, 64, -5),
                    face: 3
                },
                block_id: 1
            }
        );
    }

    #[test]
    fn roundtrip_break_payload_v2() {
        let intent = test_intent(TerrainInteractionKindV2::Break, None);

        let payload = try_encode_break_payload_v2(&intent).expect("encode break v2");
        let decoded = decode_break_payload_v2(&payload).expect("decode break v2");

        let mut expected = intent;
        expected.hit.hit_point_m = None;
        assert_eq!(decoded, expected);
    }

    #[test]
    fn roundtrip_place_payload_v2() {
        let place = TerrainPlaceIntentV2 {
            support_block_pos: (10, 64, -5),
            placement_pos: (11, 64, -5),
            block_id: BlockRuntimeId(3),
            expected_placement_empty: true,
            expected_support_solid: true,
        };
        let intent = test_intent(TerrainInteractionKindV2::Place, Some(place));

        let payload = try_encode_place_payload_v2(&intent).expect("encode place v2");
        let decoded = decode_place_payload_v2(&payload).expect("decode place v2");

        let mut expected = intent;
        expected.hit.hit_point_m = None;
        assert_eq!(decoded, expected);
    }

    #[test]
    fn encoded_break_payload_v2_fits_action_payload_limit() {
        let intent = test_intent(TerrainInteractionKindV2::Break, None);

        let payload = try_encode_break_payload_v2(&intent).expect("encode break v2");

        assert_eq!(payload.len(), COMPACT_BREAK_V2_BYTES);
        assert!(payload.len() <= ACTION_PAYLOAD_LIMIT_BYTES);
    }

    #[test]
    fn encoded_place_payload_v2_fits_action_payload_limit() {
        let place = TerrainPlaceIntentV2 {
            support_block_pos: (10, 64, -5),
            placement_pos: (11, 64, -5),
            block_id: BlockRuntimeId(3),
            expected_placement_empty: true,
            expected_support_solid: true,
        };
        let intent = test_intent(TerrainInteractionKindV2::Place, Some(place));

        let payload = try_encode_place_payload_v2(&intent).expect("encode place v2");

        assert_eq!(payload.len(), COMPACT_PLACE_V2_BYTES);
        assert!(payload.len() <= ACTION_PAYLOAD_LIMIT_BYTES);
    }

    #[test]
    fn v1_payload_is_not_valid_v2() {
        let payload = encode_break_payload_v1(
            2,
            ActionTarget {
                pos: (10, 64, -5),
                face: 3,
            },
        );

        assert_eq!(
            decode_break_payload_v2(&payload),
            Err(ActionPayloadError::InvalidV2Header)
        );
    }

    #[test]
    fn decode_rejects_invalid_v2_face_deterministically() {
        let mut intent = test_intent(TerrainInteractionKindV2::Break, None);
        intent.hit.hit_face = ClientBlockFace::PosZ;
        let mut payload = try_encode_break_payload_v2(&intent).expect("encode break v2");
        let mut cursor = TERRAIN_INTERACTION_V2_MAGIC.len();
        let _ = read_kind(&payload, &mut cursor).expect("kind");
        let _ = read_var_u32(&payload, &mut cursor, "level_id").expect("level_id");
        let _ = read_var_u32(&payload, &mut cursor, "stream_epoch").expect("stream_epoch");
        let _ = read_var_u32(&payload, &mut cursor, "input_seq").expect("input_seq");
        let _ = read_optional_var_u32(&payload, &mut cursor, "action_seq").expect("action_seq");
        let _ = read_var_u64(&payload, &mut cursor, "prediction_tx").expect("prediction_tx");
        let dependency_count =
            read_var_u32(&payload, &mut cursor, "dependency_count").expect("dependency_count");
        for _ in 0..dependency_count {
            let _ = read_var_u64(&payload, &mut cursor, "dependency").expect("dependency");
        }
        let _ = read_vec3_f32(&payload, &mut cursor, "ray_origin_m").expect("ray_origin_m");
        let _ = read_vec3_f32(&payload, &mut cursor, "ray_dir").expect("ray_dir");
        let _ = read_f32(&payload, &mut cursor, "max_distance_m").expect("max_distance_m");
        let _ = read_optional_var_u64(&payload, &mut cursor, "client_view_tick")
            .expect("client_view_tick");
        let _ = read_pos(&payload, &mut cursor, "hit_block_pos").expect("hit_block_pos");
        assert_eq!(
            payload[cursor],
            face_to_wire(ClientBlockFace::PosZ).expect("wire face")
        );
        payload[cursor] = 9;

        assert_eq!(
            decode_break_payload_v2(&payload),
            Err(ActionPayloadError::InvalidEnumValue {
                field: "hit_face",
                value: 9,
            })
        );
    }

    fn test_intent(
        kind: TerrainInteractionKindV2,
        place: Option<TerrainPlaceIntentV2>,
    ) -> TerrainInteractionIntentV2 {
        TerrainInteractionIntentV2 {
            identity: TerrainInteractionIdentityV2 {
                level_id: 7,
                stream_epoch: 9,
                input_seq: 42,
                action_seq: Some(11),
                kind,
                prediction_tx: TerrainPredictionTransactionIdV2(0xAA55),
                depends_on: vec![
                    TerrainPredictionTransactionIdV2(1),
                    TerrainPredictionTransactionIdV2(2),
                ],
            },
            ray: TerrainInteractionRayV2 {
                ray_origin_m: [0.5, 1.5, 0.5],
                ray_dir: [1.0, 0.0, 0.0],
                max_distance_m: 5.0,
                client_view_tick: Some(123),
            },
            hit: TerrainInteractionHitV2 {
                hit_block_pos: (10, 64, -5),
                hit_face: ClientBlockFace::NegX,
                hit_point_m: Some([10.0, 64.5, -4.5]),
            },
            place,
        }
    }
}
