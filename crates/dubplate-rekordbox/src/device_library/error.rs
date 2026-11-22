use super::TableKind;

/// Why a Device Library could not be written or read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeviceLibraryError {
    /// A text is longer than a DeviceSQL string can hold.
    #[error("a text of {0} bytes is longer than a DeviceSQL string can hold")]
    StringTooLong(usize),
    /// A row is too big to fit in a page, even alone.
    #[error("a row of the {table:?} table is {length} bytes, more than a page can hold")]
    RowTooLarge {
        /// The table.
        table: TableKind,
        /// The row's size in bytes.
        length: usize,
    },
    /// The library needs more pages than the file format can number.
    #[error("the library needs more pages than the format can number")]
    TooManyPages,
    /// A table's rows refer to an id the library does not hold.
    #[error("{0}")]
    DanglingReference(String),
    /// The bytes are not a DeviceSQL database.
    #[error("not a DeviceSQL database: {0}")]
    NotADatabase(String),
    /// A page is cut short, out of the file or contradicts itself.
    #[error("page {page} is malformed: {reason}")]
    MalformedPage {
        /// The page's index.
        page: super::PageIndex,
        /// What is wrong with it.
        reason: String,
    },
    /// A row does not match its table's layout.
    #[error("a row of the {table:?} table is malformed: {reason}")]
    MalformedRow {
        /// The table.
        table: TableKind,
        /// What is wrong with it.
        reason: String,
    },
    /// A string inside a row is malformed.
    #[error("the string at byte {position:#x} of its row is malformed: {reason}")]
    MalformedString {
        /// Where the string starts in its row.
        position: usize,
        /// What is wrong with it.
        reason: String,
    },
}
