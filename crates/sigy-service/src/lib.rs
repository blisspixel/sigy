//! Durable application operations. Presentation and model output are not authority.

pub use sigy_core as domain;

pub mod control;
mod error;
pub mod library;
pub mod storage;

pub use error::{Error, Result};
