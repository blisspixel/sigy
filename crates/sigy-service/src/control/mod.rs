//! Versioned, bounded local control. The catalog actor owns all durable mutations.

mod actor;
mod endpoint;
mod frame;
mod server;

use serde::{Deserialize, Serialize};

use crate::{Result, domain::money::Usd, storage::Store};

pub use server::{request, run};

pub const PROTOCOL_VERSION: u32 = 2;
pub const MAX_CLIENTS: usize = 32;
pub const MAX_REQUEST_BYTES: usize = 16 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;
pub const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub operation: Operation,
}

impl Request {
    #[must_use]
    pub const fn new(operation: Operation) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            operation,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Status {},
    SetBudget { scope: String, limit_usd: String },
    Stop {},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub result: std::result::Result<Snapshot, Failure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetView {
    pub scope: String,
    pub limit_usd: String,
    pub settled_usd: String,
    pub reserved_usd: String,
    pub available_usd: String,
    pub frozen: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceView {
    pub process_id: u32,
    pub uptime_seconds: u64,
    pub stopping: bool,
    pub maximum_clients: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u32,
    pub sqlite_version: String,
    pub provider_dispatch_available: bool,
    pub budgets: Vec<BudgetView>,
    pub service: Option<ServiceView>,
    pub captures: CaptureStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureStatus {
    pub dispatch_available: bool,
    pub scheduled: u64,
    pub active: u64,
    pub interrupted: u64,
    pub terminal: u64,
}

/// The same application operation is used by maintenance and the live controller.
/// # Errors
/// Returns validation, accounting, or catalog errors. No provider work is dispatched.
pub fn apply(store: &mut Store, operation: Operation) -> Result<Snapshot> {
    if let Operation::SetBudget { scope, limit_usd } = operation {
        let amount: Usd = limit_usd.parse()?;
        store.set_budget_limit(&scope, amount)?;
    }
    store.audit_ledger()?;
    let budgets = store
        .budgets()?
        .into_iter()
        .map(|(scope, balance)| BudgetView {
            scope,
            limit_usd: balance.limit().to_string(),
            settled_usd: balance.settled().to_string(),
            reserved_usd: balance.reserved().to_string(),
            available_usd: balance.available().to_string(),
            frozen: balance.frozen(),
        })
        .collect();
    let captures = store.capture_counts()?;
    Ok(Snapshot {
        schema_version: crate::storage::SCHEMA_VERSION,
        sqlite_version: store.sqlite_version()?,
        provider_dispatch_available: false,
        budgets,
        service: None,
        captures: CaptureStatus {
            dispatch_available: false,
            scheduled: captures.scheduled,
            active: captures.active,
            interrupted: captures.interrupted,
            terminal: captures.terminal,
        },
    })
}
