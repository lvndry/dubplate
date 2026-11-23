//! SQLite statements prepared once per connection: SQLite keeps each one compiled for the next
//! time the same SQL runs, so an edit that runs a handful of statements does not parse and plan
//! them again every time.

use std::time::SystemTime;

use dubplate_library::unix_millis;
use rusqlite::{Connection, Params, Row};

/// Runs `sql`, compiled on its first use on `connection`, and returns how many rows it changed.
pub(crate) fn execute_cached(
    connection: &Connection,
    sql: &str,
    params: impl Params,
) -> rusqlite::Result<usize> {
    connection.prepare_cached(sql)?.execute(params)
}

/// Runs the query `sql`, compiled on its first use on `connection`, and reads its first row.
pub(crate) fn query_row_cached<Value>(
    connection: &Connection,
    sql: &str,
    params: impl Params,
    read: impl FnOnce(&Row<'_>) -> rusqlite::Result<Value>,
) -> rusqlite::Result<Value> {
    connection.prepare_cached(sql)?.query_row(params, read)
}

/// The current time in Unix milliseconds, as the set store and the analysis cache record it.
pub(crate) fn now_unix_millis() -> i64 {
    i64::try_from(unix_millis(SystemTime::now())).unwrap_or(i64::MAX)
}
