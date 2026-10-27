//! Batches and snapshots are encrypted and authenticated: the server, or
//! anyone on the way, can't read them, change them, or pass one off as
//! another book's or another batch's.

use tally_sync::{SNAPSHOT_CHUNK, SyncError, SyncKey};

fn key() -> SyncKey {
    SyncKey::new("book-1", std::array::from_fn(|i| i as u8 * 3 + 1))
}

fn is_auth_failure<T: std::fmt::Debug>(r: Result<T, SyncError>) -> bool {
    matches!(r, Err(SyncError::Authentication))
}

#[test]
fn a_batch_round_trips_and_is_not_readable_in_transit() {
    let k = key();
    let plain = br#"{"changes":[{"table":"accounts","name":"Secret payroll account"}]}"#;
    let sealed = k.seal_batch("0-1-9", plain).unwrap();
    assert_eq!(k.open_batch("0-1-9", &sealed).unwrap(), plain);
    assert!(!sealed.windows(6).any(|w| w == b"payrol"), "the plaintext must not show through");
    assert_eq!(sealed.len(), 24 + plain.len() + 16, "nonce, ciphertext, tag");
    // Empty and large messages too.
    assert_eq!(k.open_batch("x", &k.seal_batch("x", b"").unwrap()).unwrap(), b"");
    let big = vec![7u8; 3_000_000];
    assert_eq!(k.open_batch("y", &k.seal_batch("y", &big).unwrap()).unwrap(), big);
}

#[test]
fn every_message_gets_its_own_nonce() {
    let k = key();
    let a = k.seal_batch("b", b"same").unwrap();
    let b = k.seal_batch("b", b"same").unwrap();
    assert_ne!(a[..24], b[..24], "a reused nonce would break the cipher");
    assert_ne!(a, b);
}

#[test]
fn the_wrong_key_fails() {
    let sealed = key().seal_batch("b", b"hello").unwrap();
    let other = SyncKey::new("book-1", [9; 32]);
    assert!(is_auth_failure(other.open_batch("b", &sealed)));
}

#[test]
fn a_mismatched_book_id_or_batch_id_fails() {
    let k = key();
    let sealed = k.seal_batch("0-1-9", b"hello").unwrap();
    // Same key, another book: Connect handing over a batch from a different book.
    let other_book = SyncKey::new("book-2", *k.key_bytes());
    assert!(is_auth_failure(other_book.open_batch("0-1-9", &sealed)));
    // Same book, a batch relabelled as another.
    assert!(is_auth_failure(k.open_batch("0-1-10", &sealed)));
    // Ids can't be shifted into each other: ("book-1", "0-1-9") is not ("book-10", "-1-9").
    let shifted = SyncKey::new("book-10", *k.key_bytes());
    assert!(is_auth_failure(shifted.open_batch("-1-9", &sealed)));
}

#[test]
fn any_changed_byte_fails_and_so_does_a_short_message() {
    let k = key();
    let sealed = k.seal_batch("b", b"a batch of changes, long enough to have a middle").unwrap();
    for i in 0..sealed.len() {
        let mut bad = sealed.clone();
        bad[i] ^= 0x01;
        assert!(is_auth_failure(k.open_batch("b", &bad)), "a flipped bit at byte {i} was accepted");
    }
    assert!(is_auth_failure(k.open_batch("b", &sealed[..sealed.len() - 1])));
    assert!(is_auth_failure(k.open_batch("b", &[])));
    assert!(is_auth_failure(k.open_batch("b", &sealed[..20])));
    let mut longer = sealed.clone();
    longer.push(0);
    assert!(is_auth_failure(k.open_batch("b", &longer)));
}

#[test]
fn a_snapshot_round_trips_in_chunks() {
    let k = key();
    let plain: Vec<u8> = (0..SNAPSHOT_CHUNK * 2 + 12_345).map(|i| (i * 31 % 251) as u8).collect();
    let sealed = k.seal_snapshot("17", &plain).unwrap();
    assert_eq!(k.open_snapshot("17", &sealed).unwrap(), plain);
    assert_eq!(k.open_snapshot("0", &k.seal_snapshot("0", b"").unwrap()).unwrap(), b"");
}

#[test]
fn a_snapshot_that_is_changed_cut_reordered_or_relabelled_fails() {
    let k = key();
    let plain: Vec<u8> = (0..SNAPSHOT_CHUNK * 2 + 100).map(|i| (i % 253) as u8).collect();
    let sealed = k.seal_snapshot("5", &plain).unwrap();
    // Wrong key, wrong book, wrong snapshot.
    assert!(is_auth_failure(SyncKey::new("book-1", [1; 32]).open_snapshot("5", &sealed)));
    assert!(is_auth_failure(SyncKey::new("book-2", *k.key_bytes()).open_snapshot("5", &sealed)));
    assert!(is_auth_failure(k.open_snapshot("6", &sealed)));
    // A flipped byte in each region.
    for at in [20, 500, SNAPSHOT_CHUNK + 1000, sealed.len() - 1] {
        let mut bad = sealed.clone();
        bad[at] ^= 0x80;
        assert!(k.open_snapshot("5", &bad).is_err(), "byte {at} changed but accepted");
    }
    // Cut short, with extra bytes, or with the chunk count changed.
    assert!(k.open_snapshot("5", &sealed[..sealed.len() - 10]).is_err());
    let mut extra = sealed.clone();
    extra.extend_from_slice(b"junk");
    assert!(k.open_snapshot("5", &extra).is_err());
    let mut fewer = sealed.clone();
    fewer[11] = 2; // claims two chunks, not three
    assert!(k.open_snapshot("5", &fewer).is_err());
    // Chunks swapped: parse the framing and put the first two in the other order.
    let mut frames = Vec::new();
    let mut rest = &sealed[12..];
    while !rest.is_empty() {
        let len = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
        frames.push(rest[..4 + len].to_vec());
        rest = &rest[4 + len..];
    }
    assert_eq!(frames.len(), 3);
    let mut swapped = sealed[..12].to_vec();
    for i in [1, 0, 2] {
        swapped.extend_from_slice(&frames[i]);
    }
    assert!(is_auth_failure(k.open_snapshot("5", &swapped)), "reordered chunks must not authenticate");
    let mut repeated = sealed[..12].to_vec();
    for i in [0, 0, 2] {
        repeated.extend_from_slice(&frames[i]);
    }
    assert!(is_auth_failure(k.open_snapshot("5", &repeated)));
    assert!(k.open_snapshot("5", b"not a snapshot").is_err());
}

#[test]
fn keys_are_random_and_not_printed() {
    let (a, b) = (SyncKey::generate().unwrap(), SyncKey::generate().unwrap());
    assert_ne!(a.key_bytes(), b.key_bytes());
    assert_ne!(a.book_id, b.book_id);
    assert!(!format!("{a:?}").contains(&format!("{:?}", a.key_bytes())));
    assert_eq!(a.book_id.len(), 32);
}
