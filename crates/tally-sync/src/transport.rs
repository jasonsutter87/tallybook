//! What the engine needs from the server, and an in-memory server for tests.
//!
//! The real client (HTTP to Tallybook Connect, see `docs/SYNC.md`) comes in a
//! later step and implements [`Transport`]; nothing else in this crate
//! changes. Everything a transport carries is already encrypted: it moves
//! bytes it can't read.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TransportError(pub String);

/// A batch as the server hands it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBatch {
    /// The book's sequence number for it, assigned by the server.
    pub seq: u64,
    /// The device that pushed it, as the server knows it.
    pub device: u16,
    pub batch_id: String,
    /// Encrypted.
    pub data: Vec<u8>,
}

pub trait Transport {
    /// Joins `book`, and returns this device's number (1 and up; the original
    /// device is 0 and never joins).
    fn join(&mut self, book: &str) -> Result<u16, TransportError>;

    /// Stores a batch and returns its sequence number. Pushing the same
    /// `(device, batch_id)` again returns the first one's sequence and stores
    /// nothing, so a retry after a lost response is safe.
    fn push_batch(&mut self, book: &str, device: u16, batch_id: &str, data: Vec<u8>) -> Result<u64, TransportError>;

    /// Batches with a sequence above `after`, oldest first.
    fn pull_batches(&mut self, book: &str, after: u64) -> Result<Vec<RemoteBatch>, TransportError>;

    /// Replaces the book's snapshot. `covers_seq` is the last batch sequence
    /// the snapshot includes.
    fn put_snapshot(&mut self, book: &str, covers_seq: u64, data: Vec<u8>) -> Result<(), TransportError>;

    /// The latest snapshot and the sequence it covers.
    fn get_snapshot(&mut self, book: &str) -> Result<Option<(u64, Vec<u8>)>, TransportError>;

    /// Says this device has applied everything up to `seq`, which lets the
    /// server drop batches every device has and a snapshot covers.
    fn ack(&mut self, book: &str, device: u16, seq: u64) -> Result<(), TransportError>;
}

// ---------------------------------------------------------------- in memory

#[derive(Default)]
struct BookData {
    next_device: u16,
    next_seq: u64,
    batches: Vec<RemoteBatch>,
    seen: BTreeMap<(u16, String), u64>,
    snapshot: Option<(u64, Vec<u8>)>,
    acked: BTreeMap<u16, u64>,
}

#[derive(Default)]
struct ServerState {
    books: BTreeMap<String, BookData>,
}

/// A server that keeps everything in memory, shared by every
/// [`MemoryTransport`] made from it.
#[derive(Clone, Default)]
pub struct MemoryServer {
    state: Arc<Mutex<ServerState>>,
}

impl MemoryServer {
    pub fn new() -> MemoryServer {
        MemoryServer::default()
    }

    /// A connection to this server for one device.
    pub fn connect(&self) -> MemoryTransport {
        MemoryTransport { server: self.clone(), fault: None, calls: 0 }
    }

    /// Every batch the server holds for `book`, as stored (encrypted).
    pub fn batches(&self, book: &str) -> Vec<RemoteBatch> {
        self.state.lock().unwrap().books.get(book).map(|b| b.batches.clone()).unwrap_or_default()
    }

    /// Replaces a stored batch's bytes: for tests of a server that tampers.
    pub fn replace_batch_data(&self, book: &str, seq: u64, data: Vec<u8>) {
        let mut state = self.state.lock().unwrap();
        if let Some(b) = state.books.get_mut(book).and_then(|b| b.batches.iter_mut().find(|b| b.seq == seq)) {
            b.data = data;
        }
    }

    /// Replaces the stored snapshot's bytes: for tests of a server that tampers.
    pub fn replace_snapshot_data(&self, book: &str, data: Vec<u8>) {
        if let Some(s) = self.state.lock().unwrap().books.get_mut(book).and_then(|b| b.snapshot.as_mut()) {
            s.1 = data;
        }
    }

    /// Drops the batches every joined device has applied and the snapshot
    /// covers, as Connect does on a schedule. Returns how many it dropped.
    pub fn prune(&self, book: &str, devices: &[u16]) -> usize {
        let mut state = self.state.lock().unwrap();
        let Some(b) = state.books.get_mut(book) else { return 0 };
        let covered = b.snapshot.as_ref().map_or(0, |s| s.0);
        let acked = devices.iter().map(|d| b.acked.get(d).copied().unwrap_or(0)).min().unwrap_or(0);
        let keep_after = covered.min(acked);
        let before = b.batches.len();
        b.batches.retain(|x| x.seq > keep_after);
        before - b.batches.len()
    }
}

/// What a [`MemoryTransport`] does when it is made to fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The call fails and has no effect: the request never arrived.
    RequestLost,
    /// The call takes effect on the server but the answer is lost.
    ResponseLost,
}

/// One device's connection to a [`MemoryServer`], which can be told to fail
/// a call, for tests of crashes and retries.
pub struct MemoryTransport {
    server: MemoryServer,
    fault: Option<(usize, Fault)>,
    calls: usize,
}

impl MemoryTransport {
    /// Makes the call numbered `after_calls` from now (0 is the next one)
    /// fail in the given way, once.
    pub fn fail(&mut self, after_calls: usize, fault: Fault) {
        self.fault = Some((self.calls + after_calls, fault));
    }

    /// How many calls this connection has made.
    pub fn calls(&self) -> usize {
        self.calls
    }

    fn call<T>(&mut self, f: impl FnOnce(&mut ServerState) -> T) -> Result<T, TransportError> {
        let n = self.calls;
        self.calls += 1;
        let fault = self.fault.filter(|(at, _)| *at == n).map(|(_, f)| f);
        if fault == Some(Fault::RequestLost) {
            return Err(TransportError("injected: request lost".into()));
        }
        let out = f(&mut self.server.state.lock().unwrap());
        if fault == Some(Fault::ResponseLost) {
            return Err(TransportError("injected: response lost".into()));
        }
        Ok(out)
    }
}

impl Transport for MemoryTransport {
    fn join(&mut self, book: &str) -> Result<u16, TransportError> {
        self.call(|s| {
            let b = s.books.entry(book.to_string()).or_default();
            b.next_device += 1;
            b.next_device
        })
    }

    fn push_batch(&mut self, book: &str, device: u16, batch_id: &str, data: Vec<u8>) -> Result<u64, TransportError> {
        self.call(|s| {
            let key = (device, batch_id.to_string());
            if let Some(seq) = s.books.get(book).and_then(|b| b.seen.get(&key)) {
                return *seq;
            }
            let b = s.books.entry(book.to_string()).or_default();
            b.next_seq += 1;
            let seq = b.next_seq;
            b.seen.insert(key, seq);
            b.batches.push(RemoteBatch { seq, device, batch_id: batch_id.to_string(), data });
            seq
        })
    }

    fn pull_batches(&mut self, book: &str, after: u64) -> Result<Vec<RemoteBatch>, TransportError> {
        self.call(|s| {
            s.books.get(book).map(|b| b.batches.iter().filter(|x| x.seq > after).cloned().collect()).unwrap_or_default()
        })
    }

    fn put_snapshot(&mut self, book: &str, covers_seq: u64, data: Vec<u8>) -> Result<(), TransportError> {
        self.call(|s| {
            s.books.entry(book.to_string()).or_default().snapshot = Some((covers_seq, data));
        })
    }

    fn get_snapshot(&mut self, book: &str) -> Result<Option<(u64, Vec<u8>)>, TransportError> {
        self.call(|s| s.books.get(book).and_then(|b| b.snapshot.clone()))
    }

    fn ack(&mut self, book: &str, device: u16, seq: u64) -> Result<(), TransportError> {
        self.call(|s| {
            let acked = s.books.entry(book.to_string()).or_default().acked.entry(device).or_insert(0);
            *acked = (*acked).max(seq);
        })
    }
}
