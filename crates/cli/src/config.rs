//! Thin wrappers over `lgtm_orchestrator`'s shared token and data-directory
//! logic, so the rest of this crate doesn't need to know it moved there.

pub use lgtm_orchestrator::token::{data_dir, resolve_token, stored_token_path};
