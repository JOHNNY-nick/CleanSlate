//! History of sessions, user exclusions and settings in SQLite (EF-20).
//!
//! The quarantine manifests stay the source of truth for the files; the
//! database is the index the UI reads, plus exclusions and settings.

use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{CoreError, CoreResult};
use crate::model::Session;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY,
    created_at  TEXT NOT NULL,
    status      TEXT NOT NULL,
    freed_bytes INTEGER NOT NULL,
    origin      TEXT NOT NULL,
    json        TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS items (
    session_id        TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    id                TEXT NOT NULL,
    kind              TEXT NOT NULL,
    original_location TEXT NOT NULL,
    status            TEXT NOT NULL,
    size_bytes        INTEGER NOT NULL,
    PRIMARY KEY (session_id, id)
);
CREATE TABLE IF NOT EXISTS exclusions (
    location TEXT PRIMARY KEY,
    added_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

fn db_error(err: rusqlite::Error) -> CoreError {
    CoreError::InvalidInput(format!("database: {err}"))
}

/// JSON text of a serde value (enums give `"completed"`, …).
fn text_of<T: serde::Serialize>(value: &T) -> CoreResult<String> {
    Ok(serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            serde_json::to_value(value)
                .map(|v| v.to_string())
                .unwrap_or_default()
        }))
}

/// The history database.
pub struct History {
    connection: Connection,
}

impl History {
    /// Opens (or creates) the database file.
    pub fn open(path: &Path) -> CoreResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CoreError::from_io(parent, e))?;
        }
        Self::init(Connection::open(path).map_err(db_error)?)
    }

    /// In-memory database (tests and simulation).
    pub fn in_memory() -> CoreResult<Self> {
        Self::init(Connection::open_in_memory().map_err(db_error)?)
    }

    fn init(connection: Connection) -> CoreResult<Self> {
        connection
            .execute_batch(&format!("PRAGMA foreign_keys = ON;{SCHEMA}"))
            .map_err(db_error)?;
        Ok(History { connection })
    }

    /// Inserts or replaces a session and its items (one transaction).
    pub fn save_session(&mut self, session: &Session) -> CoreResult<()> {
        let tx = self.connection.transaction().map_err(db_error)?;
        let id = session.id.to_string();
        tx.execute(
            "INSERT OR REPLACE INTO sessions (id, created_at, status, freed_bytes, origin, json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                session.created_at.to_rfc3339(),
                text_of(&session.status)?,
                i64::try_from(session.freed_bytes).unwrap_or(i64::MAX),
                serde_json::to_string(&session.origin)?,
                serde_json::to_string(session)?,
            ],
        )
        .map_err(db_error)?;
        tx.execute("DELETE FROM items WHERE session_id = ?1", params![id])
            .map_err(db_error)?;
        for item in &session.items {
            tx.execute(
                "INSERT INTO items (session_id, id, kind, original_location, status, size_bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    item.id,
                    text_of(&item.kind)?,
                    item.original_location,
                    text_of(&item.status)?,
                    i64::try_from(item.size_bytes).unwrap_or(i64::MAX),
                ],
            )
            .map_err(db_error)?;
        }
        tx.commit().map_err(db_error)
    }

    /// Every session, most recent first.
    pub fn sessions(&self) -> CoreResult<Vec<Session>> {
        let mut statement = self
            .connection
            .prepare("SELECT json FROM sessions ORDER BY created_at DESC")
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;
        let mut sessions = Vec::new();
        for json in rows {
            sessions.push(serde_json::from_str(&json.map_err(db_error)?)?);
        }
        Ok(sessions)
    }

    /// User exclusions (folders or keys never proposed).
    pub fn exclusions(&self) -> CoreResult<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT location FROM exclusions ORDER BY location")
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    /// Adds an exclusion (no effect if already present).
    pub fn add_exclusion(&self, location: &str) -> CoreResult<()> {
        self.connection
            .execute(
                "INSERT OR IGNORE INTO exclusions (location, added_at) VALUES (?1, ?2)",
                params![location.trim(), Utc::now().to_rfc3339()],
            )
            .map(|_| ())
            .map_err(db_error)
    }

    /// Removes an exclusion.
    pub fn remove_exclusion(&self, location: &str) -> CoreResult<()> {
        self.connection
            .execute(
                "DELETE FROM exclusions WHERE location = ?1",
                params![location.trim()],
            )
            .map(|_| ())
            .map_err(db_error)
    }

    /// Reads a setting.
    pub fn setting(&self, key: &str) -> CoreResult<Option<String>> {
        self.connection
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)
    }

    /// Writes a setting.
    pub fn set_setting(&self, key: &str, value: &str) -> CoreResult<()> {
        self.connection
            .execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map(|_| ())
            .map_err(db_error)
    }

    /// Number of items recorded for a session (index check).
    pub fn item_count(&self, session_id: &str) -> CoreResult<usize> {
        self.connection
            .query_row(
                "SELECT COUNT(*) FROM items WHERE session_id = ?1",
                params![session_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|n| usize::try_from(n).unwrap_or(0))
            .map_err(db_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LeftoverKind, Origin, QuarantineStatus, QuarantinedItem, SessionStatus};
    use chrono::{Duration, TimeZone};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn session(days_ago: i64, status: SessionStatus) -> Session {
        let created =
            Utc.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap() - Duration::days(days_ago);
        Session {
            id: Uuid::new_v4(),
            created_at: created,
            origin: Origin::App {
                app_id: "a1".into(),
            },
            items: vec![QuarantinedItem {
                id: "i1".into(),
                kind: LeftoverKind::Folder,
                original_location: r"C:\X".into(),
                quarantine_path: PathBuf::from(r"C:\Q\i1"),
                size_bytes: 10,
                sha256: None,
                original_acl: None,
                quarantined_at: created,
                restored_at: None,
                status: QuarantineStatus::Quarantined,
                error: None,
            }],
            freed_bytes: 10,
            errors: vec![],
            status,
            simulated: false,
        }
    }

    #[test]
    fn sessions_round_trip_most_recent_first() {
        let mut history = History::in_memory().unwrap();
        let old = session(10, SessionStatus::Completed);
        let new = session(1, SessionStatus::Partial);
        history.save_session(&old).unwrap();
        history.save_session(&new).unwrap();
        let mut updated = old.clone();
        updated.status = SessionStatus::Restored;
        history.save_session(&updated).unwrap();

        let all = history.sessions().unwrap();
        assert_eq!(all, vec![new, updated]);
        assert_eq!(history.item_count(&all[1].id.to_string()).unwrap(), 1);
    }

    #[test]
    fn exclusions_and_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(r"sub\cleanslate.db");
        {
            let history = History::open(&path).unwrap();
            history.add_exclusion(r"C:\Keep ").unwrap();
            history.add_exclusion(r"C:\Keep").unwrap();
            history.add_exclusion(r"HKCU\Software\Keep").unwrap();
            history.set_setting("quarantine_days", "30").unwrap();
            history.set_setting("quarantine_days", "45").unwrap();
        }
        let history = History::open(&path).unwrap();
        assert_eq!(
            history.exclusions().unwrap(),
            [r"C:\Keep", r"HKCU\Software\Keep"]
        );
        history.remove_exclusion(r"C:\Keep").unwrap();
        assert_eq!(history.exclusions().unwrap().len(), 1);
        assert_eq!(
            history.setting("quarantine_days").unwrap().as_deref(),
            Some("45")
        );
        assert_eq!(history.setting("missing").unwrap(), None);
    }
}
