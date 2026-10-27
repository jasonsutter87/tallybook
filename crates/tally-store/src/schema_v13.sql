-- Tallybook schema v13: device sync (see docs/SYNC.md).
--
-- Nothing here changes a row of the book. It adds the bookkeeping that lets
-- several devices hold copies of one book and trade changes:
--
--   sync_state   one row: this device's number, its hybrid logical clock, an
--                "applying" flag, and the last batch sequence applied.
--   sync_outbox  every change to a synced table, written by triggers in the
--                same transaction as the change, waiting to be pushed.
--   sync_rows    for each synced row, the clock reading of its last change
--                (and whether it was deleted), so the newest edit wins.
--   sync_ids     the next unused id in this device's id range, per table.
--
-- The triggers that fill the outbox are generated from each synced table's
-- columns (crates/tally-store/src/sync.rs), not written here, so a column
-- added by a later step is covered too. A book upgraded to v13 starts with an
-- empty outbox: old rows are not copied into it. A snapshot carries them to
-- a new device.

CREATE TABLE sync_state (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    -- 0 for the original device; others are assigned when they join. 8191 is
    -- reserved for rows every device derives the same way when merging.
    device      INTEGER NOT NULL DEFAULT 0 CHECK (device BETWEEN 0 AND 8190),
    -- 1 once sync is turned on for this book. Until then the triggers record
    -- nothing, so a book that never syncs pays nothing for it.
    enabled     INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    hlc_wall    INTEGER NOT NULL DEFAULT 0,
    hlc_counter INTEGER NOT NULL DEFAULT 0,
    applying    INTEGER NOT NULL DEFAULT 0 CHECK (applying IN (0, 1)),
    last_seq    INTEGER NOT NULL DEFAULT 0
);
INSERT INTO sync_state(id) VALUES (1);

CREATE TABLE sync_outbox (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    tbl TEXT NOT NULL,
    -- The primary key's values as a JSON array.
    pk  TEXT NOT NULL,
    op  TEXT NOT NULL CHECK (op IN ('insert', 'update', 'delete')),
    -- The whole row after the change, as a JSON object; null for a delete.
    row TEXT,
    hlc TEXT NOT NULL
);

CREATE TABLE sync_rows (
    tbl     TEXT NOT NULL,
    pk      TEXT NOT NULL,
    -- Clock reading of the row's last change.
    hlc     TEXT NOT NULL,
    -- Clock reading when the row was first seen; settles which of two rows
    -- that clash on a unique value is the later one.
    born    TEXT NOT NULL,
    deleted INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
    PRIMARY KEY (tbl, pk)
) WITHOUT ROWID;

CREATE TABLE sync_ids (
    tbl  TEXT PRIMARY KEY,
    next INTEGER NOT NULL
);

-- Two devices that make an account with the same code, or an invoice with the
-- same number, can't both keep it. The later one is shown with a suffix
-- ("6100-2"); this remembers what it was made with, so the name is worked out
-- again from all the rows each time and a clash that goes away leaves no
-- trace. Other devices are told the original, never the suffix.
CREATE TABLE sync_renames (
    tbl      TEXT NOT NULL,
    pk       TEXT NOT NULL,
    col      TEXT NOT NULL,
    intended TEXT NOT NULL,
    shown    TEXT NOT NULL,
    PRIMARY KEY (tbl, pk, col)
) WITHOUT ROWID;
CREATE INDEX sync_renames_intended ON sync_renames(tbl, col, intended);

-- Rows that want a value only one row can have (a second budget for the same
-- month, a duplicate bank transaction) wait here, whole, while an earlier row
-- has it. They come back if it goes away.
CREATE TABLE sync_shadow (
    tbl TEXT NOT NULL,
    pk  TEXT NOT NULL,
    row TEXT NOT NULL,
    PRIMARY KEY (tbl, pk)
) WITHOUT ROWID;

-- A void sets voided_by_id once, and the trigger still says so. While sync is
-- applying another device's changes it may replace the void that lost the
-- race with the one that won, never clear it.
DROP TRIGGER journal_entries_void_once;
CREATE TRIGGER journal_entries_void_once BEFORE UPDATE OF voided_by_id ON journal_entries
WHEN OLD.voided_by_id IS NOT NULL
 AND NOT ((SELECT applying FROM sync_state) = 1 AND NEW.voided_by_id IS NOT NULL)
BEGIN SELECT RAISE(ABORT, 'entry is already voided'); END;
