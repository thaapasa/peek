//! SQLite database flavours. Single variant today; SQLCipher / future
//! on-disk flavours would slot in here.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqliteFormat {
    /// Plain SQLite 3 database (`.sqlite` / `.sqlite3` / `.db` / `.db3`).
    Sqlite,
}

impl SqliteFormat {
    /// Display label for the Format info row. Consumed by later patches
    /// once SQLite has its own `Format` row; the info section already
    /// renders the SQLite header without one.
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::Sqlite => "SQLite 3",
        }
    }
}
