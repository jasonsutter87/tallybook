//! Encrypting batches and snapshots before they leave the device.
//!
//! XChaCha20-Poly1305 with the book's 32-byte sync key and a random 24-byte
//! nonce per message. Each message's associated data names the book and the
//! batch (or snapshot chunk) it belongs to, so Connect can't hand a device a
//! batch from another book, relabel one, or reorder a snapshot's chunks
//! without the device noticing. A message that fails to authenticate is an
//! error and is never applied.

use crate::SyncError;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

pub const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
/// Snapshots are sealed in pieces of this size, so a big book never needs one
/// huge message.
pub const SNAPSHOT_CHUNK: usize = 1 << 20;
const SNAPSHOT_MAGIC: &[u8; 8] = b"TBSNAP1\0";

/// What a device needs to read and write a book's sync data: the book id and
/// the key. Both live in the keychain and travel only in the pairing QR code.
#[derive(Clone)]
pub struct SyncKey {
    pub book_id: String,
    key: [u8; KEY_LEN],
}

impl std::fmt::Debug for SyncKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SyncKey({}, ..)", self.book_id)
    }
}

impl SyncKey {
    pub fn new(book_id: impl Into<String>, key: [u8; KEY_LEN]) -> SyncKey {
        SyncKey { book_id: book_id.into(), key }
    }

    /// A fresh random book id and key, for turning sync on for a book.
    pub fn generate() -> Result<SyncKey, SyncError> {
        let mut key = [0u8; KEY_LEN];
        let mut id = [0u8; 16];
        getrandom::fill(&mut key).map_err(random_failed)?;
        getrandom::fill(&mut id).map_err(random_failed)?;
        Ok(SyncKey { book_id: id.iter().map(|b| format!("{b:02x}")).collect(), key })
    }

    pub fn key_bytes(&self) -> &[u8; KEY_LEN] {
        &self.key
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new((&self.key).into())
    }

    /// Encrypts `plaintext` for batch `batch_id`: nonce, then ciphertext and tag.
    pub fn seal_batch(&self, batch_id: &str, plaintext: &[u8]) -> Result<Vec<u8>, SyncError> {
        self.seal(&batch_aad(&self.book_id, batch_id), plaintext)
    }

    /// Decrypts and authenticates a batch. Wrong key, wrong book, wrong batch
    /// id, or any changed byte is [`SyncError::Authentication`].
    pub fn open_batch(&self, batch_id: &str, sealed: &[u8]) -> Result<Vec<u8>, SyncError> {
        self.open(&batch_aad(&self.book_id, batch_id), sealed)
    }

    /// Encrypts a snapshot (the whole book file) in chunks.
    pub fn seal_snapshot(&self, snapshot_id: &str, plaintext: &[u8]) -> Result<Vec<u8>, SyncError> {
        let chunks: Vec<&[u8]> = if plaintext.is_empty() { vec![&[][..]] } else { plaintext.chunks(SNAPSHOT_CHUNK).collect() };
        let count = u32::try_from(chunks.len()).map_err(|_| SyncError::Format("snapshot too large".into()))?;
        let mut out = Vec::with_capacity(plaintext.len() + chunks.len() * (NONCE_LEN + TAG_LEN + 4) + 12);
        out.extend_from_slice(SNAPSHOT_MAGIC);
        out.extend_from_slice(&count.to_be_bytes());
        for (i, chunk) in chunks.iter().enumerate() {
            let sealed = self.seal(&snapshot_aad(&self.book_id, snapshot_id, i as u32, count), chunk)?;
            out.extend_from_slice(&(sealed.len() as u32).to_be_bytes());
            out.extend_from_slice(&sealed);
        }
        Ok(out)
    }

    /// Decrypts a snapshot, checking every chunk and that none is missing,
    /// repeated, or out of place.
    pub fn open_snapshot(&self, snapshot_id: &str, sealed: &[u8]) -> Result<Vec<u8>, SyncError> {
        let truncated = || SyncError::Format("snapshot is cut short".into());
        let rest = sealed.strip_prefix(SNAPSHOT_MAGIC).ok_or_else(|| SyncError::Format("not a snapshot".into()))?;
        let (count, mut rest) = rest.split_first_chunk::<4>().ok_or_else(truncated)?;
        let count = u32::from_be_bytes(*count);
        let mut out = Vec::new();
        for i in 0..count {
            let (len, tail) = rest.split_first_chunk::<4>().ok_or_else(truncated)?;
            let len = u32::from_be_bytes(*len) as usize;
            if tail.len() < len {
                return Err(truncated());
            }
            let (chunk, tail) = tail.split_at(len);
            out.extend_from_slice(&self.open(&snapshot_aad(&self.book_id, snapshot_id, i, count), chunk)?);
            rest = tail;
        }
        if !rest.is_empty() {
            return Err(SyncError::Format("snapshot has extra bytes".into()));
        }
        Ok(out)
    }

    fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, SyncError> {
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(random_failed)?;
        let ciphertext = self
            .cipher()
            .encrypt(XNonce::from_slice(&nonce), Payload { msg: plaintext, aad })
            .map_err(|_| SyncError::Format("could not encrypt".into()))?;
        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    fn open(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, SyncError> {
        if sealed.len() < NONCE_LEN + TAG_LEN {
            return Err(SyncError::Authentication);
        }
        let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
        self.cipher().decrypt(XNonce::from_slice(nonce), Payload { msg: ciphertext, aad }).map_err(|_| SyncError::Authentication)
    }
}

fn random_failed(e: getrandom::Error) -> SyncError {
    SyncError::Format(format!("no secure randomness available: {e}"))
}

/// Length-prefixed, so no book id and batch id can be run together to look like another pair.
fn aad(parts: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in parts {
        out.extend_from_slice(&(p.len() as u32).to_be_bytes());
        out.extend_from_slice(p.as_bytes());
    }
    out
}

fn batch_aad(book_id: &str, batch_id: &str) -> Vec<u8> {
    aad(&["tallybook sync batch v1", book_id, batch_id])
}

fn snapshot_aad(book_id: &str, snapshot_id: &str, index: u32, count: u32) -> Vec<u8> {
    aad(&["tallybook sync snapshot v1", book_id, snapshot_id, &index.to_string(), &count.to_string()])
}
