//! Vanilla-local MVP storage-id semantics.
//!
//! These are gameplay/storage details for vanilla behavior and are not SDK contracts.

/// Empty space in vanilla MVP storage.
pub const AIR_U8: u8 = 0;
/// MVP placeable block in vanilla behavior.
pub const STONE_U8: u8 = 1;

#[inline]
pub fn is_solid(id: u8) -> bool {
    id != AIR_U8
}

/// Placement allow-list for vanilla MVP ruleset.
#[inline]
pub fn is_place_allowed_v0(id: u8) -> bool {
    id == STONE_U8
}
