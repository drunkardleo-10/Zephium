//! Typed probabilistic answers over code-enumerated options.
//! Validation grants no disclosure, browser action, freshness or account authority.

#![forbid(unsafe_code)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]

mod contract;
mod routing;
mod schema;
mod wire;

pub use contract::*;
pub use routing::*;

#[cfg(any(test, feature = "evals"))]
pub mod evals;

/// Exact Jev model; changing it requires the recorded live evaluation gate.
pub const JEV_MODEL: &str = "jev-1.13.0";
pub const NONE_OPTION: &str = "none";
pub const MAX_CHOICE_OPTIONS: usize = 255;
pub const MAX_SCORE_LEVELS: usize = 10;
pub const MAX_QUESTIONS: usize = 64;
/// Conservative byte ceilings leave space beneath the vendor's token ceilings
/// (32k tokens for state plus the longest question, 64k per request). Live
/// ZSEM3 batches measure about 2.9 bytes per token (6,398 tokens for an
/// 18,496-byte request), so these stay near 18k and 28k tokens. They are not
/// represented as an exact vendor tokenizer count.
pub const MAX_STATE_BYTES: usize = 48 * 1024;
pub const MAX_STATE_AND_QUESTION_BYTES: usize = 52 * 1024;
pub const MAX_REQUEST_BYTES: usize = 80 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;
