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
   ids. Ids stay under `2^53`, so JavaScript reads them exactly (8,192
   devices per book).
2. **The outbox.** Triggers on every synced table write each insert, update
   and delete to a `sync_outbox` table, in the same transaction as the change
   itself, with the whole new row as JSON and a hybrid logical clock (HLC)
   timestamp. A crash can't lose a change without also losing the change.
3. **Batches.** The device bundles outbox rows into a batch, encrypts it
   (XChaCha20-Poly1305 with the book's sync key, the book id and batch
   number as associated data), and pushes it to Connect. Connect gives each
   batch the book's next sequence number.
4. **Pull and apply.** A device pulls batches after the last sequence it
   applied, decrypts them, and applies each row change with the outbox
   triggers switched off (so applying doesn't echo back), in one transaction
   per batch.
5. **Snapshots.** A new device starts from an encrypted snapshot of the book
   (the whole database, as `VACUUM INTO` writes it) plus the batches after
   it. Devices upload a fresh snapshot now and then so new devices don't
   replay years of batches, and Connect drops batches every device has
   applied that a snapshot covers.

## Merging

| Change | Rule |
|---|---|
| A journal entry or its lines | Insert if absent. Never edited, so never in conflict. |
| A void (`voided_by_id`) | First void wins by HLC. A second void of the same entry from another device is dropped, and its reversing entry is voided in turn, so the books still net to one void. |
| Any other row, updated on two devices | The newer HLC wins, for the whole row. |
| A row deleted on one device and edited on another | The delete wins if it's newer; an edit that arrives after the delete is dropped. |
| A unique value clash (two devices create an account with code `6100`, an invoice number `INV-0042`) | The later row is renamed with a suffix (`6100-2`, `INV-0042-2`) and the person is told. |
| Bank transactions | Only the device that holds a bank connection syncs it. The others receive its transactions through sync, and duplicate external ids are skipped, as file import already does. |
| The lock date and reconciliations | A change into a period that's closed on the receiving device is still applied (the other device was allowed to make it), and flagged for review. |

After each batch is applied, the device checks the books still balance
(trial balance debits equal credits). If they don't, the batch is rolled
back, sync pauses for that book, and the person is told, with nothing lost
on either side. That's a bug in this design if it ever happens, not a
normal conflict.

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
   the HLC. Tests that run two stores side by side and merge them.
2. Engine: batches, encryption, snapshots, the merge rules above, the
   balance check. Tests that fuzz two and three devices making random
   changes offline and prove they converge to the same books.
3. Connect: the endpoints, limits, and cleanup.
4. Desktop: Settings → Phone & sync: turn sync on for a book, show the QR
   code, see devices, remove one.
5. Screens that work at phone size.
6. iOS and Android builds.
