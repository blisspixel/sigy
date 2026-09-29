use rusqlite::Connection;

use crate::{Error, Result};

// Run both before migration commit and on reopen. Triggers enforce writes, while
// this audit also detects incomplete commits or rows inserted with triggers removed.
pub(crate) fn audit(connection: &Connection) -> Result<()> {
    audit_sql(connection, include_str!("integrity.sql"))
}

/// The v26 through v33 audit. Coverage still has one row and no ordinal column.
pub(crate) fn audit_single_interval(connection: &Connection) -> Result<()> {
    audit_sql(connection, include_str!("integrity_before_chunks.sql"))
}

fn audit_sql(connection: &Connection, sql: &str) -> Result<()> {
    let invalid: bool = connection.query_row(sql, [], |row| row.get(0))?;
    if invalid {
        return Err(Error::CatalogIntegrity);
    }
    Ok(())
}
