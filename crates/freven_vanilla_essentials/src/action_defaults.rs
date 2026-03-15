use freven_world_api::ActionKindId;

pub mod action_keys {
    pub const BREAK: &str = "freven.vanilla:break";
    pub const PLACE: &str = "freven.vanilla:place";
}

pub const ACTION_KIND_BLOCK_BREAK: ActionKindId = ActionKindId(1);
pub const ACTION_KIND_BLOCK_PLACE: ActionKindId = ActionKindId(2);
