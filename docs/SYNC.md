# Sync between devices

Tallybook on a phone is the whole app, with a whole copy of the book. Every
device keeps its own copy, works offline, and trades changes with the others
through Tallybook Connect. Changes are encrypted on the device before they
leave it, with a key the server never has, so Connect stores and forwards
data it can't read.

This document is the design. It's written before the code, because sync is
where bookkeeping software loses money: two copies that change on their own
must merge without dropping or doubling a cent.

## What we rely on

- **The journal never edits history.** Entries and their lines are only ever
  inserted; a mistake is reversed by a new entry, and a void sets
  `voided_by_id` once. The schema's triggers enforce it. So the most
  important data merges by union: every device's entries, all kept.
- **Everything else is small records** (accounts, contacts, budgets, rules,
  settings…) where "the latest edit wins" is what a person expects.
- **One book has one owner's plan.** Sync is for one person's (or one
  household's) devices, not for strangers editing the same book at once.

## Pieces

1. **Device ids and id ranges.** Each device that joins a book gets a device
   number `n` from Connect (the original computer is `0`). New rows on that
   device take ids from its own range, `n × 2^40` up to the next device's
   range, so two devices never create the same id. Existing rows keep their
   ids, and device 0's range starts at 1, so an existing book is unchanged.
   Ids stay under `2^53`, so JavaScript reads them exactly. That is 8,192
   ranges, of which 8,191 are for devices (0 to 8190): the last range
   belongs to no device, and holds the rows every device derives the same
   way while merging (see the void rule below). A device's next id is the
   highest it has used plus one, remembered per table, so an id is never
   handed out twice even after its row is deleted. Every insert of a row
   with an id goes through one helper, and a test fails if one doesn't.
2. **The outbox.** Triggers on every synced table write each insert, update
   and delete to a `sync_outbox` table, in the same transaction as the change
   itself, with the whole new row as JSON and a hybrid logical clock (HLC)
   timestamp. A crash can't lose a change without also losing the change.
   The triggers record nothing until sync is turned on for the book, so a
   book that never syncs pays nothing for them, and a book upgraded to the
   sync schema starts with an empty outbox: old rows are not copied into it,
   because a snapshot carries them to a new device. The same triggers keep,
   for every synced row, the clock reading of its last change (`sync_rows`),
   which is what "the newer change wins" compares.
3. **Batches.** The device bundles outbox rows into a batch, encrypts it
   (XChaCha20-Poly1305 with the book's sync key, a random 24-byte nonce, and
   the book id and batch id as associated data), and pushes it to Connect.
   Connect gives each batch the book's next sequence number. A batch id is
   the device number and the first and last outbox rows in it, so pushing the
   same rows again (after a lost response) is the same batch, which Connect
   stores once; and applying a change twice does nothing, so a retry after a
   crash anywhere converges. A batch never ends inside an entry: the lines
   that follow an entry go with it, so the receiving device always checks
   whole entries.
4. **Pull and apply.** A device pulls batches after the last sequence it
   applied, decrypts them, and applies each row change with the outbox
   triggers switched off (so applying doesn't echo back), in one transaction
   per batch. The transaction also records the batch's sequence, so a crash
   can neither apply a batch twice nor skip one. A device passes over its own
   batches when they come back. Foreign keys are not enforced while applying
   (a row can arrive before the row it points at has been settled); rows left
   pointing at nothing afterward are counted and reported. The journal's own
   triggers stay on: applying never edits an entry's date or a line's amount.
5. **Snapshots.** A new device starts from an encrypted snapshot of the book
   (the whole database, as `VACUUM INTO` writes it) plus the batches after
   it. Devices upload a fresh snapshot now and then so new devices don't
   replay years of batches, and Connect drops batches every device has
   applied that a snapshot covers.

## Merging

| Change | Rule |
|---|---|
| A journal entry or its lines | Insert if absent. Never edited, so never in conflict. |
| A void (`voided_by_id`) | The void whose reversing entry was made first (by clock, then id) wins, on every device. The other reversing entry is itself reversed, by an entry and lines whose ids every device derives the same way from the loser's id (in the reserved range), so every device ends with the same rows and the books net to one void. This is the one case where `voided_by_id` is replaced, and the trigger that forbids it allows it only while sync is applying, and only to another void, never to clear one. |
| Any other row, updated on two devices | The newer HLC wins, for the whole row. Equal readings are the same change. |
| A row deleted on one device and edited on another | Whichever happened later by the clock wins: a delete newer than the edit removes the row; an edit newer than the delete brings it back. Which arrives first does not matter. |
| A unique name clash (two devices create an account with code `6100`, an invoice number `1004`) | The row made first keeps the value; the others get `6100-2`, `6100-3`, in the order they were made, and the person is told. This is worked out again from all the rows each time, and a row remembers the value it was made with (`sync_renames`) and sends that, so devices rank the same rows the same way whatever order they arrived in, and when the clash goes away (one of the rows is deleted) the other gets its own value back. Renaming once, when the clash is first seen, would leave devices disagreeing whenever one of them never saw the clash. |
| A one-per-thing clash (a second budget for the same person and month, a second open reconciliation of an account, two links of an account to a bank, a bank transaction whose external id is already here) | There is no name to suffix, so the earliest row keeps the place and the rest wait aside whole (`sync_shadow`), and come back if the earlier one goes away. This is how duplicate bank-feed ids are skipped, as file import already skips them. |
| Bank connections | The row syncs, so every device knows the account is linked. Its `secret_ref` names an entry in one device's keychain; on any other device that entry is absent, so the connection shows as "on another device" and only the device that holds the secret runs bank sync. Everyone else receives the transactions through sync. |
| The lock date | A change into a period that's closed on the receiving device is still applied (the other device was allowed to make it), and flagged for review in the report. |

After each batch is applied, the device checks the books still balance:
every entry the batch touched sums to zero, and so do all the journal's
lines (so the trial balance's debits equal its credits). If they don't, the
batch is rolled back, sync pauses for that book, and the person is told,
with nothing lost on either side. That's a bug in this design if it ever
happens, not a normal conflict.

### What syncs

Everything in the book, except this device's own bookkeeping (`sync_state`,
`sync_outbox`, `sync_rows`, `sync_ids`, `sync_renames`, `sync_shadow`) and
the settings that belong to one machine (`device.*`, and `pref.cloud.*`,
which say where this computer backs up). Every other table is the same book
on every device: accounts, the journal, reconciliations, contacts and
documents, bank connections, links and transactions, rules, the tax center,
people, budgets, recurring bills, goals, and import links. The file-format
document lists each with why. A test fails if a table is added without
deciding which side of this line it is on, and another if a column is added
that the outbox triggers don't write; the triggers are rebuilt from the
tables' columns after every migration step.

## Keys and pairing

- Each synced book has a random 256-bit **sync key** and a random **book
  id**. Both stay in the keychain of every device that has the book.
- **Pairing a phone:** the computer shows a QR code holding the book id and
  sync key; the phone scans it. Nothing in the QR code is sent to Connect.
  (A short code typed by hand is the fallback.)
- Connect sees: the license (as today), book ids, device numbers, batch
  sizes and times. It never sees a key, a row, or a file name.
- Losing every device loses the sync key; the encrypted batches on Connect
  are then useless, which is the point. The book file and its backups are
  the recovery path, as they are today.

## On Connect

New endpoints, under the same license check as bank sync:

- `POST /v1/sync/{book}/devices`: join a book, get a device number.
- `POST /v1/sync/{book}/batches`: push an encrypted batch, get its sequence.
- `GET /v1/sync/{book}/batches?after=N`: pull batches after `N`.
- `PUT` and `GET /v1/sync/{book}/snapshot`: the latest encrypted snapshot.
- `POST /v1/sync/{book}/ack`: the last sequence this device applied.

Limits per plan: books, devices per book, and stored bytes, so one account
can't fill the server. When a plan ends, pushes and pulls stop (each device
keeps working on its own copy), and the stored batches and snapshots are
deleted with the rest of that plan's data after the grace period.

## Phones

- The phone app is the Tauri app built for iOS and Android, with the same
  Rust core and React screens, laid out for a small screen.
- Secrets on the phone: the iOS Keychain, and the Android Keystore.
- The phone's own copy of the book is encrypted on the device.
- Bank connections stay on the device that made them. Bank sync on the
  phone means "pull what the computer synced", unless the phone is the one
  that connected the bank.

## Order of work

1. Store: device id ranges, the outbox triggers, apply with triggers off,
   the HLC. Tests that run two stores side by side and merge them. (Done:
   `tally-store`, `sync.rs` and `sync_apply.rs`.)
2. Engine: batches, encryption, snapshots, the merge rules above, the
   balance check. Tests that fuzz two and three devices making random
   changes offline and prove they converge to the same books. (Done:
   `tally-sync`, over an in-memory transport with the real HTTP client a
   later step.)
3. Connect: the endpoints, limits, and cleanup.
4. Desktop: Settings → Phone & sync: turn sync on for a book, show the QR
   code, see devices, remove one.
5. Screens that work at phone size.
6. iOS and Android builds.
