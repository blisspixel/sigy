//! Durable application operations. Presentation and model output are not authority.

pub use sigy_core as domain;

pub mod control;
pub mod discovery;
mod error;
pub mod languages;
pub mod library;
mod podcast;
pub(crate) mod processing;
mod providers;
pub mod recognition;
pub mod recognizer;
pub mod recordings;
mod schedule;
pub mod sources;
pub mod storage;
pub mod translation;

pub use error::{Error, Result};
