//! Server-side vanilla action handlers.
//!
//! Responsibilities:
//! - decode and validate vanilla action payloads
//! - apply authoritative world mutations through SDK world-edit services
//! - keep break/place semantics out of engine and runner crates
//!
//! Extension guidance:
//! - keep validation deterministic and cheap
//! - prefer the vanilla-owned payload codecs in `crate::action_payloads`
//! - add one module per action kind

pub mod r#break;
pub mod place;
