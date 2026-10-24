# Tallybook API reference

Everything the app does goes through one function:

```rust
App::call(method: &str, params: serde_json::Value) -> Result<serde_json::Value, ApiError>
```

in `crates/tally-api/src/lib.rs`. A method is a dotted name such as
`invoices.create`; params and results are JSON. It is not REST: there are no
URLs, verbs or status codes, only methods, and nothing runs on a server.

The same function sits behind three doors, so they can never drift apart:

| Caller | How it calls |
|--------|--------------|
| **React UI** | `call(method, params)` in `app/src/api.ts`. In the desktop app that is the Tauri command `call` (`invoke("call", { method, params })`, `app/src-tauri/src/lib.rs`); in a browser it is the dev server. Response shapes are mirrored by hand in `app/src/types.ts`. |
| **Dev server** | `POST http://127.0.0.1:1441/api/<method>` with the params as the JSON body (empty or `null` for none). Success is `200` with the result as the body. Any error is `422` with `{"code": "...", "message": "..."}`. Development only; it binds to localhost and uses a separate keychain service. |
| **MCP server** (`tallybook-mcp`, or `tallybook-desktop --mcp`) | Curated tools (`book_status`, `accounts`, `report`, `review_inbox`, `categorize`, `record_entry`, ...) that each end in `App::call`, plus a generic tool `call` taking `{ "method": "...", "params": {...} }`. Errors come back as an `isError` tool result whose text is `"<message> (<code>)"`. See [AGENTS.md](AGENTS.md) for setup. |

Methods that take no params may be called with `null`, `{}` or no body.
Unknown fields in params are ignored; missing required fields, or fields of
the wrong type, fail with `bad_params`.

## Conventions

- **Money** is an integer number of cents (`12345` is $123.45), never a float.
  In an entry line, positive is a debit and negative a credit. Reports and
  balances are in the account's natural sign (an income account's balance is
  positive when it has earned money). Percentages that must be exact are basis
  points (`2200` is 22%).
- **Dates** are `YYYY-MM-DD` strings (no times, no zones). "Today" is the local
  date of the machine running the app.
- **Months** are `YYYY-MM` strings (`budget.*`, `household.*`).
- **Years** are integers (`2026`). `tax.*` accepts only years that have
  verified tax tables (`tax.years` lists them).
- **Ids** are positive 64-bit integers, assigned by the book. They are unique
  within their kind (accounts, entries, bank transactions, ...) and never
  reused. The param tables below name the kind (`AccountId`, `BankTxnId`,
  `ContactId`, ...); on the wire they are all plain integers.
- **Scope.** A book has a *business* side, a *household* side, or both. A
  `scope` param is `"business"` or `"household"`. Where it is optional, an
  absent scope means the whole book, unless the method says otherwise (a
  default of "the book's main side" is called out where it applies: the
  business side when the book has one). Invoices, bills, contacts and tax
  prep exist only on the business side; people, budgets, goals and recurring
  bills only on the household side.
- **Optional params** are marked `?`. For a field that can be cleared, `null`
  clears it and omitting it leaves it alone (called out where it matters:
  `cloud.set`, `settings.set`).
- **Enums** are `snake_case` strings (`"safe_harbor"`, `"married_joint"`).
- **Archive, not delete.** Accounts, contacts, people and goals are archived
  (`{ "id": n, "archived": true }`); posted entries are voided, never edited,
  so the journal stays an audit trail. A void posts a reversing entry on the
  `date` given (today when absent).
- **Atomicity.** Each call is a transaction in the store: it either fully
  applies or leaves the book unchanged. That includes calls made of several
  steps (`accounts.create` with an opening balance, `entries.post` and
  `inbox.accept` with their tags and rule, `inbox.undo`, `import.ledger`).
  `inbox.accept_suggested` applies each transaction on its own.
- **The book lock.** One book is open at a time and calls are serialised
  against it. Network calls (`bank.*`) and file work (`tax.export`,
  `cloud.backup_now`) read what they need under the lock and run outside it.

## Errors

Every failure is `{ "code": "...", "message": "..." }`. `message` is written
for people and safe to show; branch on `code`, which is one of this
vocabulary (and nothing else):

| Code | Meaning | Typical cause |
|------|---------|---------------|
| `bad_params` | The params could not be read. | Missing or mistyped field, unknown enum string, a number out of range (`tax_year` too large), an OFX statement index that doesn't exist. |
| `rule` | A bookkeeping or business rule refused the request. | An entry that doesn't balance, a locked period, a duplicate, a wrong confirmation name, an invalid CSV/OFX file, "this book isn't encrypted". |
| `not_found` | The thing named doesn't exist. | An unknown id, or a book path that isn't on disk (`book.open`, `book.delete`). |
| `no_book` | No book is open. | Any book-level method before `book.open`/`book.create`, or after the app swapped the file for one this process can't open. |
| `locked` | The book is encrypted and no key is available. | `book.open` on an encrypted book whose key isn't in this computer's keychain; use `book.unlock`. |
| `license` | The license doesn't cover this. | Creating a business book without Business, any tax prep without Business + Tax, or a tax year the license doesn't cover; also a bad or refunded key in `license.activate`. |
| `unsupported_year` | No verified tax tables for that year. | `tax.*` with a year not in `tax.years().supported`. Tallybook refuses rather than guess. |
| `unknown_method` | No such method. | A typo, or a method from a newer version. |
| `io` | Reading or writing a file failed. | Permissions, a full disk, a missing folder. |
| `keychain` | The OS keychain refused or lost a secret. | `bank.*`, `book.encrypt`, `book.delete`. |
| `bank` | A bank provider (Plaid, SimpleFIN) returned an error. | `bank.*` network calls. |
| `plaid_not_configured` | Plaid keys haven't been added. | `bank.plaid_*` before `bank.plaid_config_set`. |
| `offline` | The license server couldn't be reached. | `license.activate`/`refresh`/`deactivate` without a connection. |
| `internal` | A bug or unexpected database failure. | Report it; the book is left unchanged. |

`bank.sync` with no `connection_id` never fails because of one connection:
each connection's outcome is a `SyncReport` (below).

## Access for agents

The MCP server sorts every method into exactly one of three lists
(`crates/tally-mcp/src/lib.rs`, kept complete by a test against
`tally_api::METHODS`). The **Access** column below uses the same words:

- **read**: always allowed.
- **write**: changes the book (or the app's settings); allowed only when the
  server was started with `--write`.
- **never**: refused for agents in every mode (books' destruction, encryption
  and keys, licenses, cloud destinations, bank credentials). Do these in the
  app.

Note that "read" is about the book's contents: `book.open` changes which
book is open but not what is in it. For agents, `book.close` counts as a
write, and a few params are refused even where the method is allowed:
`settings.set` with `lock_date` (closing or reopening periods is the user's
call), `tax.export` with `dir`, and `book.open` on anything but a
`.tallybook` file. An agent session also never changes the app's recent-books
list.

---

## Book: `book.*`

`session.rs` (everyday) and `books.rs` (encryption, deletion).

| Method | Access | Params | Returns | Notes / errors |
|--------|--------|--------|---------|----------------|
| `book.status` | read | none | `{ open, path, company_name, encrypted, today, sides: [Scope] }` | Never fails; `open: false` with nothing else when no book is open. |
| `book.recent` | read | none | `[{ path, name, last_opened, encrypted }]`, newest first | Entries whose file is gone are dropped. |
| `book.default_dir` | read | none | the folder books are kept in (string) | `~/Documents/Tallybook`, or `$TALLYBOOK_DIR`. |
| `book.create` | write | `path`, `company_name`, `sides?` (`[Scope]`, default `["business"]`) | `{ path, company_name }`; the new book is open | `rule` if the name is empty, `sides` is empty, or `path` exists; `license` for a business side without a Business license. |
| `book.create_demo` | write | `path` | `{ path, company_name }` | Makes "Northwind Studio" with sample data; `rule` if `path` exists. |
| `book.open` | read | `path` | `{ path, company_name }` | `not_found`; `locked` if encrypted and the key isn't known here; `rule` for a file that isn't a Tallybook book (another program's SQLite database, refused before anything is written to it) or a book holding triggers or views Tallybook didn't create. Opening adds it to the recent list and removes leftovers of an interrupted encryption (`<book>.converting`, `<book>.previous`). |
| `book.close` | write | none | `null` | |
| `book.forget` | write | `path` | `null` | Removes it from the recent list; the file is untouched. |
| `book.add_side` | write | `scope` | `null` | A no-op if the book already has it; `license` for `business` without Business. |
| `book.backup` | write | none | path of the new dated copy (string) | Into `Backups/` beside the book. |
| `book.unlock` | never | `path`, `recovery_key`, `remember?` (default `true`) | `{ path, company_name }` | Opens an encrypted book and, with `remember`, saves its key in the keychain. `rule` for a wrong key. |
| `book.encrypt` | never | none | `{ recovery_key, plain_backups }` | Encrypts the open book in place. The recovery key is the only way back without the keychain. |
| `book.decrypt` | never | none | `null` | |
| `book.recovery_key` | never | none | the key (string) | `rule` if the book isn't encrypted. |
| `book.delete_plain_backups` | never | none | number removed | Permanently deletes unencrypted dated backups. |
| `book.delete` | never | `path`, `confirm_name`, `include_backups?` | `{ trashed, keychain_entries_removed, bank_connections_revoked, warnings }` | `confirm_name` must equal the business name; moves files to the Trash. |

## Settings: `settings.*`

| Method | Access | Params | Returns | Notes |
|--------|--------|--------|---------|-------|
| `settings.get` | read | none | `{ prefs: {key: value}, fiscal_year_start_month, lock_date }` | `lock_date` is `null` when books aren't closed. |
| `settings.set` | write | `prefs?` (`{key: value \| null}`; `null` or blank clears), `fiscal_year_start_month?` (1-12), `lock_date?` (`YYYY-MM-DD`, or `null` to clear) | `null` | `prefs` keys are limited to `company.name`, `company.email`, `company.phone`, `company.address`, `invoice.next_number`, `invoice.default_terms_days`, `invoice.default_tax_bp` and `invoice.footer`; any other key is `bad_params` and nothing is written (cloud copies, the tax profile and bank links have their own methods). Entries dated on or before `lock_date` are refused by every posting method with `rule`, and their payee, person, goal and bank matches can't change either. |

## Reports: `dashboard`, `reports.*`

All read-only. Shapes: `Dashboard`, `ProfitAndLoss`, `BalanceSheet`,
`TrialBalance`, `AgingReport` in `app/src/types.ts`.

| Method | Params | Returns |
|--------|--------|---------|
| `dashboard` | `scope?` (default: the book's main side) | Cash, income and expense for the period, recent activity, what needs attention. |
| `reports.pnl` | `from`, `to`, `scope?` | Profit and loss for the range. |
| `reports.balance_sheet` | `as_of?` (today), `scope?` | Assets, liabilities, equity. |
| `reports.trial_balance` | `as_of?` (today), `scope?` | Every account's debit or credit balance. |
| `reports.ar_aging` | `as_of?` (today), `scope?` | Who owes the business, bucketed `current`, `days_1_30`, `days_31_60`, `days_61_90`, `over_90` by days past due. |
| `reports.ap_aging` | `as_of?` (today), `scope?` | Who the business owes, same buckets. |

`scope`, where optional, covers the whole book when absent. **A/R and A/P are
business-only by nature**: invoices and bills have no household side, so
`scope: "household"` returns an empty report (no rows, zero totals) and
`"business"` or no scope returns everything.

## Accounts and entries: `accounts.*`, `entries.*`

| Method | Access | Params | Returns | Notes / errors |
|--------|--------|--------|---------|----------------|
| `accounts.list` | read | none | `[Account]` with `balance` (natural sign, as of today), `kind`, `link` (bank link or `null`), `reconcilable` | Includes archived accounts. |
| `accounts.subtypes` | read | none | `[{ value, kind }]` | The 15 subtypes: `bank`, `accounts_receivable`, `other_current_asset`, `fixed_asset`, `credit_card`, `accounts_payable`, `other_current_liability`, `long_term_liability`, `equity`, `retained_earnings`, `income`, `other_income`, `cost_of_goods_sold`, `expense`, `other_expense`. |
| `accounts.create` | write | `name`, `subtype`, `code?`, `parent_id?` (`AccountId`), `scope?` (default: main side), `flex?` (`fixed`/`usage`/`lifestyle`, household spending only), `opening_balance?` (cents, natural sign), `opening_date?` (today) | new `AccountId` | `rule` for a duplicate code, a bad parent, or a scope the book doesn't have. If the opening balance is refused (say, inside a locked period), no account is made. |
| `accounts.rename` | write | `id`, `name` | `null` | |
| `accounts.archive` | write | `id`, `archived` | `null` | `rule` while the account still has a balance. |
| `accounts.set_flex` | write | `id`, `flex` (or `null` to clear) | `null` | Household spending accounts only. |
| `accounts.opening_balance` | write | `id`, `date`, `amount` | entry id | Posts an opening balance against Opening Balance Equity; `rule` for a zero amount. |
| `accounts.register` | read | `id` | `[RegisterRow]`: every line with a running balance | |
| `entries.get` | read | `id` | `Entry` with its `lines`, `voided_by_id`, `payee_id`, `person_id` | `not_found`. |
| `entries.list` | read | `from`, `to` | `[Entry]` dated in the range | |
| `entries.post` | write | `date`, `memo`, `lines` (`[{ account_id, amount, memo? }]`, amounts summing to zero), `person_id?`, `payee_id?` (`ContactId`, feeds 1099s) | new `EntryId` | `rule` if it doesn't balance, has fewer than two lines or a zero line, or is in a locked period; `not_found` for an unknown `person_id`/`payee_id`, and then nothing is posted. |
| `entries.void` | write | `id`, `date?` (today) | `null` | Posts a reversing entry. `rule` if already void. |
| `entries.set_person` | write | `id`, `person_id` (or `null`) | `null` | Household "who was it for". `rule` for an entry in a locked period. |
| `entries.set_goal` | write | `id`, `goal_id` (or `null`) | `null` | Counts an entry toward a savings goal; the entry must touch the goal's account. `rule` for an entry in a locked period. Lives in `goals.rs` but is named for the thing it tags. |

## People and budgets (household): `people.*`, `budget.*`

| Method | Access | Params | Returns | Notes |
|--------|--------|--------|---------|-------|
| `people.list` | read | none | `[Person]`, archived last | |
| `people.create` | write | `name`, `color?` | new `PersonId` | |
| `people.update` | write | `id`, `person` (`{ name, color? }`) | `null` | Replaces name and colour. |
| `people.archive` | write | `id`, `archived` | `null` | Archived people drop out of later plans. |
| `budget.month` | read | `month?` (current) | `MonthBudget`: the plan in effect, progress against each line, pace projection | Judged as of today. |
| `budget.set` | write | `month`, `line` (`{ target, amount, direction?, tolerance_pct? }`) | budget line id | `target` is `{ "kind": "person" \| "account", "id": n }`. `direction` is `ceiling` (default, spend at most) or `floor` (put in at least). `tolerance_pct` 0-100 (default 10). Plans carry forward into later months with no plan of their own. |
| `budget.remove` | write | `month`, `target` | `null` | Drops the line from that month onward. |
| `budget.set_available` | write | `month`, `amount` (cents, or `null` to clear) | `null` | What there is to work with that month. |

## Recurring bills (household): `recurring.*`

| Method | Access | Params | Returns | Notes |
|--------|--------|--------|---------|-------|
| `recurring.list` | read | none | `[RecurringBill]` with `next_due` | |
| `recurring.create` | write | `name`, `account_id`, `cadence`, `start_date`, `amount?` (0), `varies?`, `paid_from_id?`, `person_id?`, `auto_pay?`, `match_text?` | new `RecurringBillId` | `cadence` is `{ "every": "month", "day": n }`, `{ "every": "months", "months": n, "day": d }` or `{ "every": "week" }`. |
| `recurring.update` | write | `id`, `bill` (same fields as create) | `null` | Replaces every field. |
| `recurring.set_active` | write | `id`, `active` | `null` | An inactive bill stops producing due dates. |
| `recurring.due` | read | `from?` (a week ago), `to?` (end of next month) | `[DueDate]`, each `paid`, `skipped`, `overdue`, `processing`, `due_today` or `upcoming` | |
| `recurring.mark` | write | `bill_id`, `due_date`, `status` (`paid`, `skipped`, or `null` for automatic), `entry_id?` | `null` | `due_date` must be one of the bill's real due dates. |
| `recurring.suggestions` | read | none | `[BillSuggestion]` (at most 50) | Repeat payments that look like bills. |
| `recurring.dismiss_suggestion` | write | `match_text` | `null` | Remembered so it isn't suggested again. |

## Goals and household insights: `goals.*`, `household.*`

| Method | Access | Params | Returns | Notes |
|--------|--------|--------|---------|-------|
| `goals.list` | read | none | `[Goal]` with saved so far and a projection | |
| `goals.create` | write | `name`, `target` (cents, positive), `account_id` (a household asset), `target_date?`, `starting?`, `note?` | new `GoalId` | `rule` when the store refuses the account or amounts. |
| `goals.update` | write | `id`, `goal` (same fields as create) | `null` | |
| `goals.archive` | write | `id`, `archived` | `null` | |
| `goals.contribute` | write | `id`, `from` (`AccountId`), `amount` (negative takes money out), `date?` (today) | `{ entry_id, finished }` | `finished` is true when this contribution reached the target. |
| `household.alerts` | read | `month?` (current) | `[Alert]`: near or over budget, unpaid bills | |
| `household.opportunities` | read | `month?` | spending by flex tier and where it could bend | |
| `household.score` | read | `month?` | a 300-850 style budget score with its factors | |
| `household.wins` | read | `month?` | `[Win]`: specific good news | |

## Review inbox: `inbox.*`

Imported bank transactions wait here until accepted into the books.

| Method | Access | Params | Returns | Notes |
|--------|--------|--------|---------|-------|
| `inbox.list` | read | `account_id?` | `[InboxItem]`: the transaction plus a `suggestion` (`match`, `rule` or `history`) | |
| `inbox.reviewed` | read | `account_id?` | `[ReviewedTxn]`, newest first, at most 200, with `categories` | |
| `inbox.count` | read | none | number waiting | |
| `inbox.candidates` | read | `id` (`BankTxnId`) | `[candidate]`: existing entry lines it could match | |
| `inbox.accept` | write | `id`, `account_id` (the category), `memo?`, `remember?` (false; also makes a rule), `payee_id?`, `person_id?` | the new `EntryId` | Posting, tags and rule happen together: if any fails, the transaction stays in the inbox. |
| `inbox.split` | write | `id`, `splits` (`[{ account_id, amount, memo? }]`, summing to the transaction's amount), `memo?` | `EntryId` | `rule` if the splits don't sum. |
| `inbox.match` | write | `id`, `line_id` (`LineId`) | `null` | Matches to a line already in the books instead of posting. |
| `inbox.exclude` | write | `ids` (`[BankTxnId]`) | `null` | Marks as not belonging in the books. |
| `inbox.undo` | write | `id` | `null` | Reverses an accept, match or exclude. `rule` when the entry involved is in a locked period. |
| `inbox.accept_suggested` | write | `ids` (`[BankTxnId]`) | `{ accepted, skipped }` | Applies each transaction's own suggestion; ones without are skipped, not failed. |

## Importing: `import.*`

File contents are passed as strings (`content`), not paths, except Ledger.

| Method | Access | Params | Returns | Notes |
|--------|--------|--------|---------|-------|
| `import.csv_preview` | read | `content` | `{ headers, rows, mapping }` (first rows, with a guessed `mapping`) | `rule` for an unreadable file. |
| `import.csv` | write | `content`, `mapping`, `account_id` | `ImportSummary` `{ added, updated, removed, skipped, auto_posted }` | `mapping`: `{ has_header, date_column, date_format (iso \| month_day_year \| day_month_year), description_column, amount: { kind: "single", column, negate } \| { kind: "split", debit, credit } }`. Idempotent: transactions seen before are skipped. |
| `import.ofx_preview` | read | `content` | `[{ account, count, from, to }]`, one per statement | |
| `import.ofx` | write | `content`, `index` (which statement), `account_id` | `ImportSummary` | `bad_params` for an index that doesn't exist. |
| `import.ledger_preview` | read | `path` | counts and date range | Reads a Ledger SQLite file from disk; imports nothing. |
| `import.ledger` | write | `path`, `money_account_id?` | `{ added, skipped, notes }` | Safe to repeat; an import that fails adds nothing. Lands in a new "Ledger import" account when none is given. |

## Rules: `rules.*`

Categorization rules applied to the inbox, lowest `priority` first.

| Method | Access | Params | Returns |
|--------|--------|--------|---------|
| `rules.list` | read | none | `[Rule]` |
| `rules.create` | write | `name`, `priority` (default 100), `op` (`contains` \| `equals` \| `starts_with`), `pattern`, `direction` (`any` \| `inflow` \| `outflow`), `target_account_id`, `amount_min?`, `amount_max?`, `bank_account_id?`, `rename_to?`, `auto_post`, `person_id?` | new `RuleId` |
| `rules.update` | write | `id`, `rule` (same fields as create) | `null` |
| `rules.delete` | write | `id` | `null` |

## Reconciliation: `reconcile.*`

| Method | Access | Params | Returns | Notes |
|--------|--------|--------|---------|-------|
| `reconcile.active` | read | `account_id` | the in-progress `Reconciliation` or `null` | |
| `reconcile.history` | read | `account_id` | `[Reconciliation]` | |
| `reconcile.candidates` | read | `id` (reconciliation) | `[ReconcileCandidate]`: uncleared lines | |
| `reconcile.summary` | read | `id` | `ReconcileSummary` (cleared total, difference from the statement) | |
| `reconcile.start` | write | `account_id`, `statement_date`, `statement_balance` | new `ReconciliationId` | `rule` if the account isn't a balance-sheet account or one is already in progress. |
| `reconcile.toggle` | write | `id`, `line_id`, `checked` | `null` | |
| `reconcile.toggle_many` | write | `id`, `line_ids`, `checked` | `null` | |
| `reconcile.finish` | write | `id` | `null` | `rule` unless the difference is zero. |
| `reconcile.cancel` | write | `id` | `null` | |
| `reconcile.undo_last` | write | `account_id` | `null` | |

## Contacts, invoices, bills, payments (business)

`contacts.*`, `invoices.*`, `bills.*` and `payments.*` are the business
sales cycle. The prefix split is historical: `payments.*` settles both
invoices and bills, so it has its own prefix, and lives in `bills.rs`.

### `contacts.*`

| Method | Access | Params | Returns |
|--------|--------|--------|---------|
| `contacts.list` | read | none | `[Contact]` |
| `contacts.get` | read | `id` | `Contact` |
| `contacts.create` | write | `kind` (`customer` \| `vendor` \| `both`), `name`, `company?`, `email?` (one plain address; `rule` otherwise), `phone?`, `address?`, `notes?` | new `ContactId` |
| `contacts.update` | write | `id`, `contact` (same fields as create) | `null` |
| `contacts.archive` | write | `id`, `archived` | `null` |

### `invoices.*`

Money owed to the business. A draft can be edited and deleted; a finalized
invoice can only be voided.

| Method | Access | Params | Returns | Notes |
|--------|--------|--------|---------|-------|
| `invoices.list` | read | none | `[InvoiceSummary]` (status `draft`, `open`, `partial`, `paid`, `overdue`, `void`) | |
| `invoices.get` | read | `id` | `InvoiceDetail` with lines and applied payments | |
| `invoices.create` | write | `customer_id`, `issue_date`, `due_date`, `memo`, `tax_rate_bp`, `lines` (`[{ description, quantity_milli, unit_price, account_id, taxable }]`; `quantity_milli` 1000 = 1) | new `InvoiceId` (a draft) | |
| `invoices.update` | write | `id`, `draft` (same fields as create) | `null` | `rule` unless a draft. |
| `invoices.delete` | write | `id` | `null` | Drafts only. |
| `invoices.finalize` | write | `id` | the posted `EntryId` | Posts the invoice to A/R. |
| `invoices.void` | write | `id`, `date?` (today) | `null` | Reverses the posting. |

### `bills.*`

Money the business owes.

| Method | Access | Params | Returns |
|--------|--------|--------|---------|
| `bills.list` | read | none | `[BillSummary]` |
| `bills.get` | read | `id` | `{ summary, lines, payments }` |
| `bills.create` | write | `vendor_id`, `number`, `issue_date`, `due_date`, `memo`, `lines` (`[{ description, amount, account_id }]`) | new `BillId` |
| `bills.void` | write | `id`, `date?` (today) | `null` |

### `payments.*`

| Method | Access | Params | Returns | Notes |
|--------|--------|--------|---------|-------|
| `payments.list` | read | `kind?` (`received` \| `made`), `contact_id?`, `from?` (the beginning), `to?` (today) | `[PaymentSummary]` | |
| `payments.receive` | write | `contact_id`, `date`, `account_id` (deposit to), `method`, `reference`, `memo`, `applications` (`[{ document_id, amount }]`, invoice ids) | new `PaymentId` | `rule` when an application can't be applied (the store checks amounts against the document). |
| `payments.pay` | write | same as receive, with `account_id` the account paid from and `document_id` a bill id | new `PaymentId` | |
| `payments.void` | write | `id`, `date?` (today) | `null` | The invoices or bills become unpaid again. |

## Tax: `tax.*`

Worksheets and estimates only; nothing is filed. Business side only. Every
method except `tax.years`, `tax.lines`, `tax.set_payee` and `tax.set_contact`
needs the Business + Tax license (`license`), and a method about a particular
year needs that tax year covered. A year with no verified tables fails with
`unsupported_year`. `year?` defaults to the current year.

| Method | Access | Params | Returns | Notes |
|--------|--------|--------|---------|-------|
| `tax.years` | read | none | `{ supported, current, licensed, disclaimer }` | |
| `tax.lines` | read | none | `[{ line, number, label, help }]`: the Schedule C lines | |
| `tax.profile` | read | none | `TaxProfile` | Defaults when never set. |
| `tax.set_profile` | write | `filing_status` (`single` \| `married_joint` \| `married_separate` \| `head_of_household`), `method` (`safe_harbor` \| `current_year`), `other_income`, `w2_wages`, `withholding` (cents), `last_year_tax?`, `last_year_agi?`, `projected_profit?`, `home_office_sqft` (0 = none), `state_rate_bp` (0-2000) | `null` | All fields except the `?` ones are required. `rule` for a state rate over 20%. |
| `tax.schedule_c` | read | `year?` | `{ report, home_office_sqft, disclaimer }` | |
| `tax.set_line` | write | `account_id`, `line?` (a Schedule C line id; `null` clears the mapping) | `null` | |
| `tax.confirm_all` | write | `year?` | `null` | Confirms every suggested account-to-line mapping. |
| `tax.estimate` | read | `year?` | the profile, profit used, `estimate`, `safe_harbor`, `plan_annual`, `plan_method` (`safe_harbor` \| `current_year`), `quarters` (each `status`: `paid`, `nothing_due`, `late`, `due_soon`, `upcoming`), `payments`, `disclaimer` | |
| `tax.record_payment` | write | `tax_year`, `quarter` (1-4), `jurisdiction` (`federal` \| `state`), `date`, `amount`, `account_id` | new `TaxPaymentId` | `bad_params` for a `tax_year` outside the 32-bit range; `rule` for a quarter outside 1-4, a zero amount, or an account that can't pay. Also posts the payment as an entry. |
| `tax.void_payment` | write | `id` | `null` | |
| `tax.1099` | read | `year?` | `Report1099`: payees, amounts, who crosses the threshold | |
| `tax.set_payee` | write | `entry_id`, `contact_id` (or `null`) | `null` | Free of the tax license: it is everyday bookkeeping. `rule` for an entry in a locked period. |
| `tax.set_contact` | write | `id`, `is_1099`, `w9_on_file`, `legal_name?`, `tin_last4?` | `null` | Only the last four digits of a TIN are stored. |
| `tax.export` | write | `year?`, `dir?` (an existing folder) | path of the new `Tax <year>/` folder, in `dir` or beside the book | Writes CSVs, two PDFs and a README, none of them encrypted. An encrypted book needs `dir` (`rule` without it), so the package never lands silently beside the book. Text cells that a spreadsheet would read as a formula start with `'`. `io` on a write failure. |

## Bank connections: `bank.*`

Linking and syncing bank feeds (Plaid, SimpleFIN). All except the first and
`bank.feed_accounts`/`bank.suggest_opening` are refused for agents. Provider
credentials live in the OS keychain, never in the book.

| Method | Access | Params | Returns |
|--------|--------|--------|---------|
| `bank.connections` | read | none | `[Connection]` each with its `links` |
| `bank.feed_accounts` | read | `connection_id` | the provider's account list (network) |
| `bank.suggest_opening` | read | `account_id` | `{ date, amount, bank_balance }`: a suggested opening balance |
| `bank.plaid_config_get` | never | none | `{ configured, environment, client_id_hint }` |
| `bank.plaid_config_set` | never | Plaid `client_id`, `secret`, `environment` | `null` |
| `bank.plaid_start` | never | none | a hosted-link `{ link_token, url }` |
| `bank.plaid_relink_start` | never | `connection_id` | a hosted-link, for repairing a connection |
| `bank.plaid_poll` | never | `link_token`, `connection_id?` | `{ status: "waiting" \| "exited" \| "linked", ... }` |
| `bank.simplefin_connect` | never | `setup_token` (its claim URL and the access URL it returns must be https to SimpleFIN Bridge) | `{ status: "linked", connection_id, institution, accounts }` |
| `bank.simplefin_reconnect` | never | `connection_id`, `setup_token` | `null` |
| `bank.link` | never | `connection_id`, `external_account_id`, `name`, `start_date`, `mask?`, and `account_id` (existing) or `new_account` (`{ name, subtype, scope? }`) | `{ link_id, account_id }` |
| `bank.unlink` | never | `link_id` | `null` |
| `bank.disconnect` | never | `connection_id` | `null` |
| `bank.sync` | write | `connection_id?` (every connection when absent) | `[SyncReport]` |

`SyncReport` is `{ connection_id, institution, ok, error, imported: ImportSummary }`.
Syncing everything records a connection that can't sync (a credential missing
from the keychain, the provider refusing) as `ok: false` with the reason in
`error`, and carries on with the rest; naming one `connection_id` returns that
connection's failure as an error instead. The network call runs without the
book lock.

## License: `license.*`

| Method | Access | Params | Returns | Notes |
|--------|--------|--------|---------|-------|
| `license.status` | read | none | `{ tier, name, key_hint, email, activated_on, tax_years, active, checked_on, syncs, *_checkout, manage_url }` | `tier` is `free`, `personal`, `business` or `tax`. `active` is whether the yearly plan was paid up when last checked (`null` if never checked); `syncs` is whether bank and phone sync are included. |
| `license.activate` | never | `key` | `license.status` | `license` for a bad, ended or refunded key; `offline`. |
| `license.refresh` | never | none | `license.status` | Asks Lemon Squeezy about the current key's plan: picks up a renewal (adding that year's tax season) or a plan change, or marks the plan ended. An ended plan keeps the tier; only sync stops. `offline`. |
| `license.deactivate` | never | none | `null` | Frees the keys for another computer. |

## Cloud copies: `cloud.*`

Encrypted copies of the book into a synced folder and/or a Git repository.
Only an encrypted book can be copied.

| Method | Access | Params | Returns | Notes |
|--------|--------|--------|---------|-------|
| `cloud.status` | read | none | `{ encrypted, folder, git, git_has_remote, auto, keep, last, last_error, due, suggestions }` | |
| `cloud.set` | never | `folder?`, `git?` (a path, or `null` to clear; omit to leave unchanged), `auto?`, `keep?` (1-100) | `null` | `rule` if `folder` isn't a folder or `git` isn't a repository. |
| `cloud.backup_now` | never | none | `{ folder, git }`, each `{ ok, path, detail }` or `null` | `rule` unless the book is encrypted and a destination is set. One snapshot is taken under the lock; copying, `git` and pruning run outside it. |

---

## Notes for maintainers

- Method names are a public contract: agents, docs and the UI call them by
  name, so they are never renamed. A few groupings are historical rather than
  tidy: `entries.set_goal` is routed to `goals.rs`; `payments.*` is in
  `bills.rs`; `book.delete`/`unlock`/`encrypt`/`decrypt`/`recovery_key`/
  `delete_plain_backups` are in `books.rs`, the rest of `book.*` in
  `session.rs`; `dashboard` is a method with no dot.
- To add a method: add its arm in the area's router, add it to
  `tally_api::METHODS` and to exactly one of `READ`, `WRITE`, `NEVER` in
  `crates/tally-mcp/src/lib.rs`, and document it here. Tests fail if any of
  those is missed.
- A new error code needs a row in the table above; use an existing one first.
- Changing a response shape means updating every caller in the same change:
  `app/src/types.ts` and its pages, `tally-mcp`, the e2e tests and these docs.
