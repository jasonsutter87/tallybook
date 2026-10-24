//! Pushing, pulling and snapshots.

use crate::{SyncError, SyncKey, Transport};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tally_store::Store;
use tally_store::sync::{ApplyReport, RowChange};

/// About how many row changes go in one batch (a batch never ends inside an
/// entry, so it can run a few lines over).
pub const MAX_BATCH_ROWS: usize = 500;

/// What goes inside the encryption.
#[derive(Serialize, Deserialize)]
struct BatchBody {
    version: u32,
    device: u16,
    changes: Vec<RowChange>,
}

const BATCH_VERSION: u32 = 1;

/// What one round of [`sync_once`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub batches_pushed: usize,
    pub changes_pushed: usize,
    pub batches_pulled: usize,
    /// The merge, summed over every batch applied: renames and clashes the person should be told about.
    pub applied: ApplyReport,
}

fn merge(into: &mut ApplyReport, from: ApplyReport) {
    into.applied += from.applied;
    into.dropped += from.dropped;
    into.renamed.extend(from.renamed);
    into.clashes_dropped.extend(from.clashes_dropped);
    into.voids_merged += from.voids_merged;
    into.in_closed_period.extend(from.in_closed_period);
    into.dangling_references += from.dangling_references;
}

/// Trades changes with the server: pushes this device's outbox, pulls and
/// applies the other devices' batches, and acknowledges. Safe to run again at
/// any point after a failure; running it twice in a row does nothing the
/// second time.
pub fn sync_once(store: &mut Store, transport: &mut dyn Transport, key: &SyncKey) -> Result<SyncReport, SyncError> {
    let mut report = SyncReport::default();
    let (batches, changes) = push_outbox(store, transport, key)?;
    report.batches_pushed = batches;
    report.changes_pushed = changes;
    let (pulled, applied) = pull_and_apply(store, transport, key)?;
    report.batches_pulled = pulled;
    report.applied = applied;
    transport.ack(&key.book_id, store.sync_device()?, store.sync_last_seq()?.max(0) as u64)?;
    Ok(report)
}

/// Encrypts the outbox into batches and pushes them, forgetting each from the
/// outbox only once the server has it. A retry after a lost response pushes
/// the same batch under the same id, which the server stores once.
pub fn push_outbox(store: &Store, transport: &mut dyn Transport, key: &SyncKey) -> Result<(usize, usize), SyncError> {
    if !store.sync_enabled()? {
        return Err(SyncError::NotEnabled);
    }
    let device = store.sync_device()?;
    let (mut batches, mut changes) = (0, 0);
    loop {
        let rows = store.sync_outbox_batch(MAX_BATCH_ROWS)?;
        let (Some(first), Some(last)) = (rows.first(), rows.last()) else { break };
        let (first_seq, last_seq) = (first.seq, last.seq);
        let batch_id = format!("{device}-{first_seq}-{last_seq}");
        let body = BatchBody { version: BATCH_VERSION, device, changes: rows.into_iter().map(|r| r.change).collect() };
        let plaintext = serde_json::to_vec(&body).map_err(|e| SyncError::Format(e.to_string()))?;
        let sealed = key.seal_batch(&batch_id, &plaintext)?;
        transport.push_batch(&key.book_id, device, &batch_id, sealed)?;
        store.sync_outbox_done(last_seq)?;
        batches += 1;
        changes += body.changes.len();
    }
    Ok((batches, changes))
}

/// Pulls the batches after the last one applied and applies them, each in its
/// own transaction that also records its sequence. A batch that fails to
/// authenticate, or that would unbalance the books, stops the pull there with
/// an error and changes nothing.
pub fn pull_and_apply(store: &mut Store, transport: &mut dyn Transport, key: &SyncKey) -> Result<(usize, ApplyReport), SyncError> {
    if !store.sync_enabled()? {
        return Err(SyncError::NotEnabled);
    }
    let device = store.sync_device()?;
    let after = store.sync_last_seq()?.max(0) as u64;
    let mut pulled = transport.pull_batches(&key.book_id, after)?;
    pulled.sort_by_key(|b| b.seq);
    let mut report = ApplyReport::default();
    let mut applied_batches = 0;
    for batch in pulled.into_iter().filter(|b| b.seq > after) {
        let seq = i64::try_from(batch.seq).map_err(|_| SyncError::Format("sequence out of range".into()))?;
        if batch.device == device {
            // Our own batch, coming back. Nothing to apply; just move past it.
            store.sync_apply(&[], Some(seq))?;
            continue;
        }
        let plaintext = key.open_batch(&batch.batch_id, &batch.data)?;
        let body: BatchBody = serde_json::from_slice(&plaintext).map_err(|e| SyncError::Format(e.to_string()))?;
        if body.version != BATCH_VERSION {
            return Err(SyncError::Format(format!("a batch of version {}, which this version doesn't know", body.version)));
        }
        if body.device != batch.device {
            return Err(SyncError::Authentication);
        }
        merge(&mut report, store.sync_apply(&body.changes, Some(seq))?);
        applied_batches += 1;
    }
    Ok((applied_batches, report))
}

// ---------------------------------------------------------------- snapshots

/// A directory only this user can read, for the plain copy of a book on its
/// way into or out of encryption.
fn private_temp_dir() -> Result<PathBuf, SyncError> {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|e| SyncError::Format(format!("no secure randomness available: {e}")))?;
    let name: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let dir = std::env::temp_dir().join(format!("tallybook-sync-{name}"));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&dir).map_err(|e| SyncError::Format(format!("could not make a temporary folder: {e}")))?;
    Ok(dir)
}

fn snapshot_id(covers_seq: u64) -> String {
    covers_seq.to_string()
}

/// Uploads an encrypted snapshot of the whole book, so a new device doesn't
/// replay every batch. Taken with `VACUUM INTO` (a consistent copy), through
/// a private temporary folder that is removed afterward.
pub fn create_snapshot(store: &Store, transport: &mut dyn Transport, key: &SyncKey) -> Result<u64, SyncError> {
    if !store.sync_enabled()? {
        return Err(SyncError::NotEnabled);
    }
    let covers = store.sync_last_seq()?.max(0) as u64;
    let dir = private_temp_dir()?;
    let result = (|| {
        let file = dir.join("snapshot.tallybook");
        store.sync_snapshot_to(&file)?;
        let plain = std::fs::read(&file).map_err(|e| SyncError::Format(format!("could not read the snapshot: {e}")))?;
        let sealed = key.seal_snapshot(&snapshot_id(covers), &plain)?;
        transport.put_snapshot(&key.book_id, covers, sealed)?;
        Ok(covers)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Starts this device's copy of a book: joins it (getting a device number),
/// downloads and decrypts the latest snapshot into a new book file at `dest`,
/// and opens it as that device with an empty outbox, positioned to pull the
/// batches after the snapshot. Run [`sync_once`] next to catch up.
pub fn join_from_snapshot(transport: &mut dyn Transport, key: &SyncKey, dest: &Path) -> Result<Store, SyncError> {
    if dest.exists() {
        return Err(SyncError::Format(format!("{} already exists", dest.display())));
    }
    let (covers, sealed) = transport.get_snapshot(&key.book_id)?.ok_or(SyncError::NoSnapshot)?;
    let plain = key.open_snapshot(&snapshot_id(covers), &sealed)?;
    let device = transport.join(&key.book_id)?;
    std::fs::write(dest, plain).map_err(|e| SyncError::Format(format!("could not write {}: {e}", dest.display())))?;
    let opened = Store::open(dest).and_then(|mut s| {
        s.sync_become_device(device, i64::try_from(covers).unwrap_or(i64::MAX))?;
        Ok(s)
    });
    match opened {
        Ok(store) => Ok(store),
        Err(e) => {
            let _ = std::fs::remove_file(dest);
            Err(e.into())
        }
    }
}
