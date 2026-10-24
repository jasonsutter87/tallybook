//! Unit tests for `sync.rs` that need no second device. The two- and
//! three-device scenarios are in `tests/sync.rs`.

use crate::sync::*;

#[test]
fn clock_readings_round_trip_and_sort_like_their_text() {
    let a = Hlc { wall_ms: 1_700_000_000_000, counter: 2, device: 1 };
    let b = Hlc { wall_ms: 1_700_000_000_000, counter: 10, device: 0 };
    let c = Hlc { wall_ms: 1_700_000_000_001, counter: 0, device: 0 };
    for h in [a, b, c] {
        assert_eq!(Hlc::parse(&h.to_string()), Some(h));
    }
    assert!(a < b && b < c);
    assert!(a.to_string() < b.to_string() && b.to_string() < c.to_string(), "text order is clock order");
    assert_eq!(Hlc::parse("nonsense"), None);
}

#[test]
fn device_ranges_are_disjoint_and_under_2_pow_53() {
    assert_eq!(id_range(0).0, 1, "existing books keep their ids");
    let mut previous_hi = 0;
    for device in [0, 1, 2, 3, 100, MAX_DEVICE, MERGE_DEVICE] {
        let (lo, hi) = id_range(device);
        assert!(lo > previous_hi && lo <= hi);
        previous_hi = hi;
        assert!(hi < 1 << 53, "JavaScript must read ids exactly");
    }
}

/// Every `INSERT INTO <table with an id>` in the workspace's non-test source
/// goes through `insert_with_id`, with `id` as its last column. A new insert
/// that forgets would take SQLite's own next rowid, which two devices can
/// both choose.
#[test]
fn every_insert_into_a_numbered_table_takes_its_id_from_the_device_range() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(&crates).unwrap() {
        let src = entry.unwrap().path().join("src");
        if src.is_dir() {
            collect_rs(&src, &mut sources);
        }
    }
    assert!(sources.len() > 10, "found the sources");
    let tables: Vec<&str> = id_tables().collect();
    let mut sites = 0;
    for path in sources {
        if path.file_name().is_some_and(|n| n == "sync.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        // Test modules at the bottom of a file are exempt.
        let text = text.split("#[cfg(test)]\nmod tests").next().unwrap();
        for (at, _) in text.match_indices("INSERT INTO ") {
            let rest = &text[at + "INSERT INTO ".len()..];
            let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !tables.contains(&name.as_str()) {
                continue;
            }
            sites += 1;
            let before = &text[at.saturating_sub(200)..at];
            assert!(before.contains("insert_with_id("), "{}: INSERT INTO {name} bypasses insert_with_id", path.display());
            let columns = &rest[..rest.find(')').unwrap()];
            assert!(columns.ends_with(", id"), "{}: INSERT INTO {name} must end its columns with id", path.display());
        }
    }
    assert!(sites >= 20, "expected to find the insert sites, found {sites}");
}

fn collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}
