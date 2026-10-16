# Developing Tallybook

Rust core · Tauri desktop app · React UI · SQLite · bank feeds via Plaid,
SimpleFIN, or CSV/OFX import.

## Principles

- **Real double-entry.** Every entry balances or it isn't saved.
- **Integer money.** Amounts are `i64` cents; floats never touch a balance.
  Release builds keep overflow checks on.
- **Append-only ledger.** Entries are never edited or deleted, only voided by
  a reversing entry. SQLite triggers enforce this even if the API is bypassed.
- **No closing entries.** Retained earnings are derived from the fiscal-year
  boundary at report time.
- **Your data, your keys.** Bank credentials live in the OS keychain, never in
  the book file, and go only to the provider. See [BANK_FEEDS.md](BANK_FEEDS.md).
- **Taxes: prepare and explain, never file.** Tax tables are year-versioned,
  sourced, and pinned by tests against IRS-published figures. See
  [TAXES.md](TAXES.md).

## Layout

```
crates/
  tally-core/       pure accounting: money, accounts, entries, reports, reconciliation,
                    invoice math, feed records, categorization rules, tax math (no I/O)
  tally-store/      SQLite persistence and bookkeeping rules (lock date, voids,
                    reconciliation, bank-feed inbox, invoices/bills/payments, tax center)
  tally-feed/       Plaid (Hosted Link + /transactions/sync), SimpleFIN, CSV, OFX/QFX
  tally-api/        the application layer: one JSON dispatcher, App::call(method, params)
  tally-devserver/  serves tally-api over HTTP for browser development
app/                React UI (Vite)
app/src-tauri/      desktop shell: a single IPC command forwarding to tally-api
```

The desktop app and the dev server both forward to the same `App::call`, so
what you test in a browser is exactly what ships.

## Getting started

Requirements: Rust (stable), Node 20+. On Linux you also need the WebKitGTK
and D-Bus development packages (see `.github/workflows/release.yml`).

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

## Checks

CI runs exactly these. Use current stable Rust, since a newer clippy flags
more lints:

```sh
cargo clippy --workspace --exclude tallybook-desktop --all-targets -- -D warnings
cargo test --workspace --exclude tallybook-desktop
cd app && npm run build
```

## Releasing

See [RELEASING.md](RELEASING.md). Push a version tag and GitHub Actions
builds draft installers for macOS, Windows and Linux.
