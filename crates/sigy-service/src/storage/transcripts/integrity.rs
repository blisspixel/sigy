use rusqlite::Connection;

use crate::{Error, Result};

// Run both before migration commit and on reopen. Triggers enforce writes, while
// this audit also detects incomplete commits or rows inserted with triggers removed.
pub(crate) fn audit(connection: &Connection) -> Result<()> {
    let invalid: bool =
        connection.query_row(include_str!("integrity.sql"), [], |row| row.get(0))?;
    if invalid {
        return Err(Error::CatalogIntegrity);
    }
    Ok(())
}
