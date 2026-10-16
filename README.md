# Tallybook

Open-source, self-hosted small-business bookkeeping. A QuickBooks alternative
where your books are a single SQLite file on your own machine.

**Stack:** Rust (Tauri v2 desktop) · React · SQLite · bank feeds via Plaid,
SimpleFIN, or CSV/OFX import.

## Principles

- **Real double-entry.** Every entry balances or it isn't saved.
- **Integer money.** Amounts are `i64` cents; floats never touch a balance.
  Release builds keep overflow checks on.
- **Append-only ledger.** Entries are never edited or deleted, only voided by
  a reversing entry. SQLite triggers enforce this even if the API is bypassed.
- **No closing entries.** Retained earnings are derived from the fiscal-year
  boundary at report time.
- **Your data, your keys.** Bank credentials (Plaid/SimpleFIN tokens) live in
  your OS keychain and are never sent anywhere but the provider.

## Layout

```
crates/
  tally-core/    pure accounting domain: money, accounts, entries, reports, reconciliation (no I/O)
  tally-store/   SQLite persistence + bookkeeping rules (lock date, voids, reconciliation workflow)
```

## Roadmap (hardest → easiest)

- [x] **P1: Ledger engine.** Money type, chart of accounts, balanced entries,
      void-by-reversal, lock date, account registers, trial balance, P&L,
      balance sheet with fiscal-year retained earnings, statement reconciliation
      (start / check off / finish / cancel / undo)
- [ ] **P2: Bank feeds.** `BankFeed` provider trait; Plaid (Hosted Link in the
      system browser plus `/transactions/sync` cursors), SimpleFIN, CSV/OFX import;
      dedupe; match-to-existing; categorization rules; auto-clear on match
- [ ] **P3: Desktop app.** Tauri shell, React UI (registers, review-transactions
      inbox, reconcile screen, reports), keychain secrets, auto-backup
- [ ] **P4: Sales & purchases.** Customers/vendors, invoices (PDF), payments,
      A/R + A/P aging, bills, sales tax
- [ ] **P5: Niceties.** Budgets, attachments/receipts, QuickBooks/CSV import,
      multi-currency, year-end export for your accountant

## Development

```sh
cargo test
cargo clippy --all-targets
```

## License

AGPL-3.0-or-later
