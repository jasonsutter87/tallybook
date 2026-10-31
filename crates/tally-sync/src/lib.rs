//! Device sync for Tallybook (see `docs/SYNC.md`).
//!
//! `tally-store` records every change to a synced table in an outbox and
//! knows how to merge another device's changes in. This crate moves them:
//!
//! - [`SyncKey`] encrypts a batch of changes, or a snapshot of the whole
//!   book, with XChaCha20-Poly1305, so the server only ever holds bytes it
//!   can't read or alter.
//! - [`Transport`] is what the engine needs from the server: push, pull,
//!   snapshots, joining, acknowledging. [`MemoryServer`] is an in-memory one
//!   for tests; the HTTP client for Tallybook Connect is a later step and
//!   implements the same trait.
//! - [`sync_once`] pushes the outbox, pulls and applies what other devices
//!   pushed, and acknowledges. Every step is safe to repeat, so a crash or a
//!   lost response anywhere followed by a retry converges.
//! - [`create_snapshot`] and [`join_from_snapshot`] start a new device.

mod crypto;
mod engine;
mod transport;

pub use crypto::{KEY_LEN, SNAPSHOT_CHUNK, SyncKey};
pub use engine::{MAX_BATCH_ROWS, SyncReport, create_snapshot, join_from_snapshot, pull_and_apply, push_outbox, sync_once};
pub use transport::{Fault, MemoryServer, MemoryTransport, RemoteBatch, Transport, TransportError};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Store(#[from] tally_store::StoreError),
    #[error("could not reach the sync server: {0}")]
    Transport(#[from] TransportError),
    /// A batch or snapshot didn't decrypt: the wrong key, another book's data,
    /// a mislabelled message, or a changed byte. It is never applied.
    #[error("sync data failed its authenticity check (wrong key, wrong book, or tampered with)")]
    Authentication,
    #[error("unreadable sync data: {0}")]
    Format(String),
    #[error("sync isn't turned on for this book")]
    NotEnabled,
    #[error("the server has no snapshot of this book yet")]
    NoSnapshot,
}
