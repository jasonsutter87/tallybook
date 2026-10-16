//! Book encryption at rest (SQLCipher, AES-256) and recovery keys.
//!
//! Each encrypted book has a random 256-bit key. It's handed to SQLCipher
//! as a raw key (`x'…'`), so no password stretching is involved: the key
//! *is* the entropy. Users see it once as a **recovery key**: Crockford
//! base32 with a checksum, so a mistyped character is caught instead of
//! producing a confusing "wrong key".

use crate::{Result, Store, StoreError, rule};
use rusqlite::Connection;
use std::path::Path;
use tally_core::bank::fnv1a64;

pub const KEY_LEN: usize = 32;
pub type BookKey = [u8; KEY_LEN];

const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const PREFIX: &str = "TB1";

/// A fresh key from the operating system's secure random source.
pub fn generate_key() -> Result<BookKey> {
    let mut key = [0u8; KEY_LEN];
    getrandom::fill(&mut key).map_err(|e| StoreError::Rule(format!("no secure randomness available: {e}")))?;
    Ok(key)
}

/// True when the file exists and isn't a plain SQLite database.
pub fn is_encrypted_file(path: &Path) -> std::io::Result<bool> {
    use std::io::Read;
    let mut header = [0u8; 16];
    let mut f = std::fs::File::open(path)?;
    let n = f.read(&mut header)?;
    Ok(n == 16 && &header != SQLITE_HEADER)
}

fn hex(key: &BookKey) -> String {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

/// SQLCipher's raw-key literal.
pub(crate) fn key_literal(key: &BookKey) -> String {
    format!("x'{}'", hex(key))
}

fn checksum(key: &BookKey) -> [u8; 2] {
    let h = fnv1a64(key).to_be_bytes();
    [h[0], h[1]]
}

/// `TB1-XXXXX-XXXXX-…` (55 base32 characters: 256-bit key + 16-bit checksum).
pub fn recovery_key(key: &BookKey) -> String {
    let mut bytes = key.to_vec();
    bytes.extend_from_slice(&checksum(key));
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for b in bytes {
        buf = (buf << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buf << (5 - bits)) & 31) as usize] as char);
    }
    let groups: Vec<String> = out.as_bytes().chunks(5).map(|c| String::from_utf8_lossy(c).into_owned()).collect();
    format!("{PREFIX}-{}", groups.join("-"))
}

/// Parses a recovery key, tolerating case, spaces, dashes and the usual
/// look-alikes (O→0, I/L→1).
pub fn parse_recovery_key(input: &str) -> Result<BookKey> {
    let cleaned: String = input.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect();
    let body = cleaned.strip_prefix(PREFIX).unwrap_or(&cleaned);
    let mut bytes = Vec::with_capacity(34);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in body.chars() {
        let c = match c {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        let v = ALPHABET.iter().position(|&a| a as char == c).ok_or_else(|| StoreError::Rule(format!("'{c}' isn't used in recovery keys")))?;
        buf = (buf << 5) | v as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    if bytes.len() != KEY_LEN + 2 {
        return rule("that recovery key is the wrong length; check for missing or extra characters");
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes[..KEY_LEN]);
    if bytes[KEY_LEN..] != checksum(&key) {
        return rule("that recovery key has a typo (the checksum doesn't match)");
    }
    Ok(key)
}

/// Applies `key` to a fresh connection and proves it's right.
pub(crate) fn unlock(conn: &Connection, key: &BookKey) -> Result<()> {
    conn.pragma_update(None, "key", key_literal(key))?;
    match conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)) {
        Ok(_) => Ok(()),
        Err(_) => rule("that key doesn't open this book"),
    }
}

impl Store {
    /// Opens an encrypted book. Errors with a rule message on a wrong key.
    pub fn open_encrypted(path: impl AsRef<Path>, key: &BookKey) -> Result<Store> {
        let conn = Connection::open(path)?;
        unlock(&conn, key)?;
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        Store::init(conn, Some(*key))
    }

    pub fn is_encrypted(&self) -> bool {
        self.key.is_some()
    }

    pub fn key(&self) -> Option<&BookKey> {
        self.key.as_ref()
    }

    /// Row counts used to prove a converted copy holds the same data.
    pub fn fingerprint(&self) -> Result<(i64, i64, i64)> {
        let count = |t: &str| self.conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get::<_, i64>(0));
        Ok((count("accounts")?, count("journal_entries")?, count("journal_lines")?))
    }

    /// Writes an encrypted copy of the whole book to `dest` (which must not exist).
    pub fn export_encrypted(&self, dest: &Path, key: &BookKey) -> Result<()> {
        self.export_to(dest, &key_literal(key))
    }

    /// Writes a plain (unencrypted) copy of the whole book to `dest`.
    pub fn export_plain(&self, dest: &Path) -> Result<()> {
        self.export_to(dest, "")
    }

    fn export_to(&self, dest: &Path, key: &str) -> Result<()> {
        if dest.exists() {
            return rule(format!("{} already exists", dest.display()));
        }
        let version: i64 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        self.conn.execute("ATTACH DATABASE ?1 AS export KEY ?2", rusqlite::params![dest.to_string_lossy(), key])?;
        let result = (|| -> Result<()> {
            self.conn.query_row("SELECT sqlcipher_export('export')", [], |_| Ok(()))?;
            // sqlcipher_export copies schema and data, not the schema version.
            self.conn.execute_batch(&format!("PRAGMA export.user_version = {version};"))?;
            Ok(())
        })();
        self.conn.execute("DETACH DATABASE export", [])?;
        if result.is_err() {
            let _ = std::fs::remove_file(dest);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_keys_round_trip_and_catch_typos() {
        let key = generate_key().unwrap();
        let rk = recovery_key(&key);
        assert!(rk.starts_with("TB1-"));
        assert_eq!(rk.chars().filter(|c| c.is_ascii_alphanumeric()).count(), 3 + 55);
        assert_eq!(parse_recovery_key(&rk).unwrap(), key);
        // Forgiving about formatting.
        assert_eq!(parse_recovery_key(&rk.to_lowercase().replace('-', " ")).unwrap(), key);
        // A single changed character is caught.
        let mut chars: Vec<char> = rk.chars().collect();
        let i = chars.len() - 3;
        chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
        let typo: String = chars.into_iter().collect();
        assert!(parse_recovery_key(&typo).is_err());
        assert!(parse_recovery_key("TB1-SHORT").is_err());
        assert!(parse_recovery_key("TB1-#####").is_err());
    }

    #[test]
    fn keys_are_random() {
        assert_ne!(generate_key().unwrap(), generate_key().unwrap());
    }
}
