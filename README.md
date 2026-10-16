# Tallybook

**Open-source, self-hosted bookkeeping for small businesses.** A QuickBooks
alternative where your books are one file on your own computer.

Rust core · Tauri desktop app · React UI · SQLite · bank feeds via Plaid,
SimpleFIN, or CSV/OFX import.

## What it does

- **Home dashboard**: cash on hand, card balances, money in/out, what's owed
  to you and by you, six-month cash flow, and where the money went.
- **Bank feeds & review inbox**: transactions arrive from Plaid, SimpleFIN or
  a statement file. Each one comes with a suggestion: a *match* to something
  you already entered, a *rule*, or *what you picked last time*. Accept,
  split, match or exclude, all undoable. Tick "remember" and it becomes a rule.
- **Invoices**: drafts, issuing, tax, printable/PDF invoices, partial
  payments. Recorded payments auto-match the bank deposit when it arrives.
- **Bills**: enter what you owe, pay from checking or a card, track due dates.
- **Reconciliation**: check off a statement until the difference is $0.00.
- **Reports**: Profit & Loss, Balance Sheet, Trial Balance, A/R and A/P aging.
- **Accountant-grade underneath**: real double-entry, a full chart of
  accounts, manual journal entries, a lock date for closed periods.

## Principles

- **Real double-entry.** Every entry balances or it isn't saved.
- **Integer money.** Amounts are `i64` cents; floats never touch a balance.
  Release builds keep overflow checks on.
- **Append-only ledger.** Entries are never edited or deleted, only voided by
  a reversing entry. SQLite triggers enforce this even if the API is bypassed.
- **No closing entries.** Retained earnings are derived from the fiscal-year
  boundary at report time.
- **Your data, your keys.** Bank credentials live in your OS keychain, never
  in the book file, and go only to the provider. See
  [docs/BANK_FEEDS.md](docs/BANK_FEEDS.md).

## Layout

```
crates/
  tally-core/       pure accounting: money, accounts, entries, reports, reconciliation,
                    invoice math, feed records, categorization rules (no I/O)
  tally-store/      SQLite persistence and bookkeeping rules (lock date, voids,
                    reconciliation, bank-feed inbox, invoices/bills/payments)
  tally-feed/       Plaid (Hosted Link + /transactions/sync), SimpleFIN, CSV, OFX/QFX
  tally-api/        the application layer: one JSON dispatcher, App::call(method, params)
  tally-devserver/  serves tally-api over HTTP for browser development
app/                React UI (Vite)
app/src-tauri/      desktop shell: a single IPC command forwarding to tally-api
```

The desktop app and the dev server both forward to the same `App::call`, so
what you test in a browser is exactly what ships.

## Development

Requirements: Rust (stable), Node 20+.

```sh
cargo test                      # all backend tests
cd app && npm install

# Desktop app
npm run tauri dev

# Or in a browser, against the real backend
npm run devserver               # terminal 1: API on 127.0.0.1:1441
npm run dev                     # terminal 2: UI on http://localhost:1440
```

Set `TALLYBOOK_DIR` to keep development books out of `~/Documents/Tallybook`.
"Explore with sample data" on the welcome screen builds a realistic sample
business with six months of history.

## Building a release

```sh
cd app && npm run tauri build
```

## License

[AGPL-3.0-or-later](LICENSE). You're free to use, modify and self-host it.
If you offer a modified version to others over a network, share your changes.
