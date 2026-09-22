//! Durable application operations. Presentation and model output are not authority.

pub use sigy_core as domain;

pub mod control;
pub mod discovery;
mod error;
pub mod library;
mod podcast;
pub mod recordings;
pub mod sources;
pub mod storage;

pub use error::{Error, Result};
