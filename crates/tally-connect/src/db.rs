//! The one SQLCipher database: licenses we have seen with items, and the
//! items themselves. It holds no books and no transactions.

use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("cannot read the database: CONNECT_DB_KEY is wrong, or the file is not a Tallybook Connect database")]
    BadKey,
    #[error("the database is from a newer tally-connect (schema {0}); please upgrade")]
    TooNew(i64),
}

/// Append-only: never edit a step that has shipped, add a new one.
const MIGRATIONS: &[(i64, &str)] = &[(
    1,
    "CREATE TABLE licenses (
        key_hash      TEXT PRIMARY KEY,
        license_key   TEXT NOT NULL,
        last_valid_at INTEGER,
        last_status   TEXT NOT NULL,
        invalid_since INTEGER
     ) STRICT;
     CREATE TABLE items (
        handle       TEXT PRIMARY KEY,
        license_hash TEXT NOT NULL,
        item_id      TEXT NOT NULL,
        access_token TEXT NOT NULL,
        created_at   INTEGER NOT NULL
     ) STRICT;
     CREATE INDEX items_by_license ON items(license_hash);",
)];

pub struct LicenseRow {
    pub key_hash: String,
    pub license_key: String,
    pub invalid_since: Option<i64>,
}

pub struct ItemRow {
    pub handle: String,
    pub access_token: String,
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path, key: &str) -> Result<Db, DbError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "key", key)?;
        // A wrong key only shows up on the first read.
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)).map_err(|_| DbError::BadKey)?;
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.pragma_update(None, "secure_delete", true)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > MIGRATIONS.last().map_or(0, |m| m.0) {
            return Err(DbError::TooNew(version));
        }
        for &(step, sql) in MIGRATIONS {
            if version < step {
                let tx = conn.unchecked_transaction()?;
                tx.execute_batch(sql)?;
                tx.pragma_update(None, "user_version", step)?;
                tx.commit()?;
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Db { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Records a license seen valid, ending any lapse.
    pub fn license_valid(&self, key_hash: &str, license_key: &str, now: i64) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO licenses (key_hash, license_key, last_valid_at, last_status, invalid_since)
             VALUES (?1, ?2, ?3, 'active', NULL)
             ON CONFLICT(key_hash) DO UPDATE SET last_valid_at = ?3, last_status = 'active', invalid_since = NULL",
            params![key_hash, license_key, now],
        )?;
        Ok(())
    }

    /// Notes that Lemon Squeezy says the plan has ended; the first sighting
    /// starts the grace period.
    pub fn license_lapsed(&self, key_hash: &str, now: i64) -> rusqlite::Result<()> {
        self.conn().execute(
            "UPDATE licenses SET last_status = 'lapsed', invalid_since = COALESCE(invalid_since, ?2) WHERE key_hash = ?1",
            params![key_hash, now],
        )?;
        Ok(())
    }

    pub fn delete_license(&self, key_hash: &str) -> rusqlite::Result<()> {
        self.conn().execute("DELETE FROM licenses WHERE key_hash = ?1", [key_hash])?;
        Ok(())
    }

    pub fn count_items(&self, license_hash: &str) -> rusqlite::Result<u32> {
        self.conn().query_row("SELECT count(*) FROM items WHERE license_hash = ?1", [license_hash], |r| r.get(0))
    }

    /// Stores an item unless the license is at `max`. The check and the
    /// insert happen under one lock so concurrent exchanges can't overshoot.
    pub fn insert_item_capped(&self, handle: &str, license_hash: &str, item_id: &str, access_token: &str, now: i64, max: u32) -> rusqlite::Result<bool> {
        let conn = self.conn();
        let n: u32 = conn.query_row("SELECT count(*) FROM items WHERE license_hash = ?1", [license_hash], |r| r.get(0))?;
        if n >= max {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO items (handle, license_hash, item_id, access_token, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![handle, license_hash, item_id, access_token, now],
        )?;
        Ok(true)
    }

    /// The real token behind `handle`, only if `license_hash` owns it.
    pub fn token_for(&self, handle: &str, license_hash: &str) -> rusqlite::Result<Option<String>> {
        self.conn()
            .query_row("SELECT access_token FROM items WHERE handle = ?1 AND license_hash = ?2", params![handle, license_hash], |r| r.get(0))
            .optional()
    }

    pub fn delete_item(&self, handle: &str) -> rusqlite::Result<()> {
        self.conn().execute("DELETE FROM items WHERE handle = ?1", [handle])?;
        Ok(())
    }

    pub fn licenses_with_items(&self) -> rusqlite::Result<Vec<LicenseRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT l.key_hash, l.license_key, l.invalid_since FROM licenses l
             WHERE EXISTS (SELECT 1 FROM items i WHERE i.license_hash = l.key_hash)",
        )?;
        stmt.query_map([], |r| Ok(LicenseRow { key_hash: r.get(0)?, license_key: r.get(1)?, invalid_since: r.get(2)? }))?.collect()
    }

    pub fn items_of(&self, license_hash: &str) -> rusqlite::Result<Vec<ItemRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT handle, access_token FROM items WHERE license_hash = ?1")?;
        stmt.query_map([license_hash], |r| Ok(ItemRow { handle: r.get(0)?, access_token: r.get(1)? }))?.collect()
    }

    /// The stored invalid-since of a license (`None` while it is in good standing).
    pub fn invalid_since(&self, key_hash: &str) -> rusqlite::Result<Option<i64>> {
        self.conn().query_row("SELECT invalid_since FROM licenses WHERE key_hash = ?1", [key_hash], |r| r.get(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tc-db-{name}-{}.db", uuid::Uuid::new_v4()))
    }

    #[test]
    fn wrong_key_is_refused_and_data_is_encrypted_at_rest() {
        let p = temp("key");
        {
            let db = Db::open(&p, "right").unwrap();
            assert!(db.insert_item_capped("tbc-1", "lic", "item", "SECRET-TOKEN-XYZ", 1, 5).unwrap());
        }
        assert!(matches!(Db::open(&p, "wrong"), Err(DbError::BadKey)));
        let db = Db::open(&p, "right").unwrap();
        assert_eq!(db.token_for("tbc-1", "lic").unwrap().as_deref(), Some("SECRET-TOKEN-XYZ"));
        assert_eq!(db.token_for("tbc-1", "other").unwrap(), None);
        drop(db);
        let raw = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).to_string();
        assert!(!raw.contains("SECRET-TOKEN-XYZ") && !raw.contains("SQLite format"));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", p.display()));
        }
    }

    #[test]
    fn cap_is_enforced_per_license() {
        let p = temp("cap");
        let db = Db::open(&p, "k").unwrap();
        assert!(db.insert_item_capped("a", "lic", "i1", "t1", 1, 2).unwrap());
        assert!(db.insert_item_capped("b", "lic", "i2", "t2", 1, 2).unwrap());
        assert!(!db.insert_item_capped("c", "lic", "i3", "t3", 1, 2).unwrap());
        assert!(db.insert_item_capped("d", "other", "i4", "t4", 1, 2).unwrap());
        drop(db);
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", p.display()));
        }
    }
}
