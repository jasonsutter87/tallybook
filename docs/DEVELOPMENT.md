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
- **A file that explains itself.** A book is a SQLite database that says
  what it is (`PRAGMA application_id`, `user_version`). The schema only
  grows, one append-only `schema_vN.sql` at a time, and
  [FILE_FORMAT.md](FILE_FORMAT.md) explains every table to someone without
  the app. Change the schema, change that document.
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
what you test in a browser is exactly what ships. Every method, its params
and its errors are documented in [API.md](API.md).

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
To look around a realistic sample business with six months of history,
ask the devserver for one (the app itself no longer offers it):

```sh
curl -s -X POST 127.0.0.1:1441/api/book.create_demo \
  -H 'content-type: application/json' -d '{"path": "/tmp/Northwind sample"}'
```

## Checks

CI runs exactly these. Use current stable Rust, since a newer clippy flags
more lints:

```sh
cargo clippy --workspace --exclude tallybook-desktop --all-targets -- -D warnings
cargo test --workspace --exclude tallybook-desktop
cd app && npm run build
cd app && npm run test:e2e
```

### Performance

`crates/tally-api/tests/big_book.rs` builds a book with 10,000 entries and
2,000 transactions waiting in Review, then times each screen's main call
against a 500ms budget. It's ignored in a plain `cargo test` because it's
slow to build; CI runs it in release mode:

```sh
cargo test --release -p tally-api --test big_book -- --ignored --nocapture
```

### End-to-end tests

`app/e2e/` drives the real UI against the real backend with Playwright, the
way the browser preview pairs them. It starts its own dev server and Vite on
ports 1441 and 1440 (stop yours first; it refuses to reuse them, so a test
run can never touch your own books) and keeps everything in `app/e2e/.data`.

- `smoke.spec.ts` opens every screen, light and dark, and fails on console
  errors, error banners and serious accessibility problems (axe-core, WCAG AA).
- `flows.spec.ts` does what people do: make a book, review the inbox, invoice
  and get paid, plan a month, export a tax package, and the license gates.

`npm run test:e2e:ui` opens Playwright's UI to watch or debug a run. The first
time, install the browser with `npx playwright install chromium`.

## Releasing

See [RELEASING.md](RELEASING.md). Push a version tag and GitHub Actions
builds draft installers for macOS, Windows and Linux.
