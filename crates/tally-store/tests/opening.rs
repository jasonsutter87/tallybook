//! Opening files that may not be Tallybook books: other programs' SQLite
//! databases are refused before anything is written to them, and books
//! carrying triggers Tallybook didn't create are refused.

use rusqlite::Connection;
use std::path::PathBuf;
use tally_store::*;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tally-opening-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn tables(path: &std::path::Path) -> Vec<String> {
    let c = Connection::open(path).unwrap();
    let mut stmt = c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
}

#[test]
fn another_programs_database_is_refused_and_left_untouched() {
    let d = dir("foreign");
    let path = d.join("notes.db");
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT); INSERT INTO notes(body) VALUES ('hello');").unwrap();
    }
    let before = std::fs::read(&path).unwrap();

    assert!(matches!(Store::open(&path), Err(StoreError::Rule(_))));
    assert_eq!(std::fs::read(&path).unwrap(), before, "not a byte changed");
    assert_eq!(tables(&path), ["notes"]);
    let mode: String = Connection::open(&path).unwrap().query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "delete", "the journal mode is the file's own");
    assert!(!d.join("notes.db-wal").exists());

    // Another program's application id is refused the same way.
    let tagged = d.join("tagged.db");
    Connection::open(&tagged).unwrap().execute_batch("PRAGMA application_id = 42; CREATE TABLE accounts (id);").unwrap();
    assert!(matches!(Store::open(&tagged), Err(StoreError::Rule(_))));
}

#[test]
fn new_files_and_real_books_open() {
    let d = dir("books");
    let path = d.join("new.tallybook");
    let mut s = Store::open(&path).unwrap();
    s.seed_default_chart().unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    assert!(s.account_by_code("4000").unwrap().is_some());
}

#[test]
fn a_book_with_a_trigger_tallybook_didnt_create_is_refused() {
    let d = dir("trigger");
    let path = d.join("shared.tallybook");
    let mut s = Store::open(&path).unwrap();
    s.seed_default_chart().unwrap();
    drop(s);
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TRIGGER sneaky AFTER INSERT ON journal_lines BEGIN UPDATE journal_lines SET amount = amount * 2 WHERE id = NEW.id; END;")
        .unwrap();
    let Err(StoreError::Rule(msg)) = Store::open(&path) else { panic!("opened a book with an unknown trigger") };
    assert!(msg.contains("sneaky"), "{msg}");
}
