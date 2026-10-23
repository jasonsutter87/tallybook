# The Tallybook book file

This document is for someone holding a `.tallybook` file and nothing else:
no Tallybook app, no network, maybe no idea what Tallybook was. It explains
what the file is, how to open it, what every table means, and how to get
your numbers out of it with plain SQL.

Everything here is derived from the schema files in
`crates/tally-store/src/schema*.sql` and the code that writes them. Those
files are the authority; where this document and they disagree, they win.

Written for schema version 12 (Tallybook 0.2.x, 2026). Later versions only
ever add to the schema, so everything below stays true for newer books.

## What the file is

A `.tallybook` file is an ordinary **SQLite 3 database**
(<https://sqlite.org/fileformat.html>), one book per file. A book holds a
business, a household, or both.

- **Plain books** start with the 16 bytes `SQLite format 3\0`. Any SQLite
  tool opens them: `sqlite3 "My Business.tallybook"`.
- **Encrypted books** are SQLCipher 4 databases (see
  [Encrypted books](#encrypted-books)). The whole file, header included,
  looks like random bytes.
- **Identity.** From schema 12 on, `PRAGMA application_id;` returns
  `1413565529` (`0x54414C59`, the ASCII letters `TALY`). It's stored in bytes
  68–71 of the SQLite header, so a hex dump of a plain book shows `TALY` at
  offset `0x44`, and the Unix `file` command prints
  `application id 1413565529`. Books from before schema 12 have `0` there
  until a newer Tallybook opens them. Tallybook refuses, before writing
  anything to it, a file with another application id, or one with id `0`
  that has tables but not Tallybook's (`accounts`, `journal_entries`,
  `settings`), and a book holding triggers or views its migrations didn't
  create.
- **Schema version.** `PRAGMA user_version;` is the schema version, 1 to 12
  as of this writing. Each version is one file of SQL, applied in order:
  `schema.sql` is v1, `schema_v2.sql` is v2, and so on. Each file's header
  says what it adds. Tallybook refuses to open a book whose version is newer
  than it knows.
- **Text encoding** is UTF-8 (`PRAGMA encoding;`).
- **Journal mode** is WAL while Tallybook has the book open, so you may see
  `-wal` and `-shm` files next to it. They hold recent writes. Copy a book
  only while Tallybook is closed, or copy all three files together; SQLite
  folds them back in when the book is next opened. Opening a book in any
  SQLite tool creates these two files too, even just to read. To explore
  safely, work on a copy.

### What is *not* in the file

On purpose, the book never holds:

- **Encryption keys.** They live in the operating system's keychain (below).
- **Bank credentials** (Plaid access tokens, SimpleFIN access URLs). The
  `bank_connections.secret_ref` column names a keychain entry instead.
- **Full taxpayer ID numbers.** `contacts.tin_last4` keeps the last four
  digits only; the full number belongs on the W-9 and the IRS filing.

## Encrypted books

Encryption is SQLCipher 4 (<https://www.zetetic.net/sqlcipher/>), AES-256
with HMAC-SHA512 page authentication, using **SQLCipher 4's default
settings** unchanged:

| Setting | Value |
|---|---|
| `cipher_page_size` | 4096 |
| `cipher_hmac_algorithm` | `HMAC_SHA512` |
| `cipher_kdf_algorithm` | `PBKDF2_HMAC_SHA512` |
| `kdf_iter` | 256000 (unused for the key itself; see below) |
| `cipher_plaintext_header_size` | 0 |

A test (`encrypted_books_use_plain_sqlcipher_4_settings` in
`crates/tally-store/tests/crypto.rs`) pins these, because SQLCipher stores
none of them in the file: a book only opens with the settings it was
written with.

Each encrypted book has its own random **256-bit key**, given to SQLCipher
as a *raw key* (the `x'…'` form), so there is no password and no password
stretching: the key is the secret. To open a book with the `sqlcipher`
command-line tool, given its key as 64 hexadecimal digits:

```sql
PRAGMA cipher_compatibility = 4;
PRAGMA key = "x'0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'";
SELECT count(*) FROM journal_entries;
```

To make a plain copy you can open with any SQLite tool:

```sql
ATTACH DATABASE 'plain copy.tallybook' AS plain KEY '';
SELECT sqlcipher_export('plain');
DETACH DATABASE plain;
```

(`sqlcipher_export` copies tables, not the header fields; on the plain copy
run `PRAGMA user_version = 12; PRAGMA application_id = 1413565529;`, using
the version the encrypted book reported, before Tallybook opens it.)

### Where the key is

1. **The recovery key.** When a book is encrypted, Tallybook shows its key
   once as a recovery key, for the owner to print or store. It looks like
   `TB1-0C512-60Z4R-PK8ET-29585-EQK5D-HSQN0-C8HYB-9V95B-PAWW1-HYETQ-EAPQR`:
   the prefix `TB1` (format 1), then 55 characters of Crockford base32
   (alphabet `0123456789ABCDEFGHJKMNPQRSTVWXYZ`, no I, L, O or U) in groups
   of five. The 55 characters encode 34 bytes, most significant bit first:
   the 32-byte key followed by the first 2 bytes of the key's SHA-256 as a
   typo check. The final 3 bits are zero padding. Case, spaces and dashes
   don't matter, and O, I and L are read as 0, 1 and 1. This Python turns a
   recovery key back into the hex key for `PRAGMA key`:

   ```python
   import hashlib, sys

   ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"  # Crockford base32

   def recovery_key_to_hex(recovery_key):
       text = "".join(c for c in recovery_key.upper() if c.isalnum())
       if text.startswith("TB1"):
           text = text[3:]
       text = text.replace("O", "0").replace("I", "1").replace("L", "1")
       bits = "".join(format(ALPHABET.index(c), "05b") for c in text)
       data = bytes(int(bits[i:i + 8], 2) for i in range(0, 34 * 8, 8))
       key, check = data[:32], data[32:]
       if hashlib.sha256(key).digest()[:2] != check or int(bits[272:] or "0", 2):
           sys.exit("the recovery key has a typo")
       return key.hex()

   print(recovery_key_to_hex(sys.argv[1]))
   ```

   The example above decodes to
   `030a11181f262d343b424950575e656c737a81888f969da4abb2b9c0c7ced5dc`.

2. **The keychain.** On a computer where the book was opened, its key is in
   the operating system's credential store (macOS Keychain, Windows
   Credential Manager, or the freedesktop Secret Service on Linux) under
   service `app.tallybook` (`app.tallybook.dev` for development builds),
   account `book-key:<uuid>`, as 64 lowercase hex digits. Keys aren't tied
   to file names; `keys.json` (below) lists the uuids to try. On macOS:
   `security find-generic-password -s app.tallybook -a "book-key:<uuid>" -w`.

Without the key or the recovery key, an encrypted book can't be read. That
is the point of it.

## Conventions

- **Money** is an `INTEGER` count of **cents** (minor units; Tallybook is
  US-dollar only). `123456` is $1,234.56. No column ever holds a float.
- **Journal line signs.** In `journal_lines.amount`, **positive is a debit
  and negative is a credit**, and the lines of every entry sum to exactly
  zero. Every other money column is a plain positive amount unless its
  description says otherwise.
- **Natural sign.** People read balances in an account's *natural* sign:
  assets and expenses grow with debits, so their natural balance is the sum
  of their lines; liabilities, equity and income grow with credits, so their
  natural balance is minus the sum. A credit card you owe $50 on has lines
  summing to −5000 and a natural balance of 5000.
- **Dates** are `TEXT` in ISO 8601, `YYYY-MM-DD`, with no time zone; they
  sort and compare as text. Months are `YYYY-MM`.
- **Timestamps** (`created_at`, `imported_at` and similar) are
  `YYYY-MM-DD HH:MM:SS` in UTC, from SQLite's `datetime('now')`. They record
  when a row was written, not when the money moved.
- **Booleans** are `INTEGER` 0 or 1.
- **Rates** are integer **basis points** (`tax_rate_bp`, 825 = 8.25%) and
  **quantities** are integer **thousandths** (`quantity_milli`, 1500 = 1.5
  hours). Derived amounts round half away from zero.
- **Enumerations** are lowercase `snake_case` text, checked by the schema
  where the list is closed.

## The ledger: how the books work

Tallybook is double-entry bookkeeping. Everything with a money effect is a
**journal entry** (`journal_entries`) with two or more **lines**
(`journal_lines`), each line moving an amount into or out of one
**account** (`accounts`). Invoices, bills, payments and bank transactions
are human-facing records that each point at the journal entry that carries
their money; reports read only the journal.

**The ledger is append-only.** Triggers in `schema.sql` refuse to delete
entries or lines, or to change an entry's date or a line's account or
amount. A mistake is fixed by **voiding**: Tallybook posts a new entry with
every line negated (`source = 'void'`, `reverses_id` = the original), and
sets the original's `voided_by_id` to it. Both stay in the books and cancel
out, so **sum every line, voided or not**, and the totals are right.
To list only live entries, skip rows where `voided_by_id` or `reverses_id`
is set.

**Two sides.** A book keeps a business side, a household side, or both
(`settings` key `book.sides`). Every account belongs to one side
(`accounts.scope`), and **each side balances on its own**. When one payment
mixes the two (the business card buys groceries), Tallybook adds two
bridge lines so both sides still sum to zero: an owner's draw (business
account `3100`) paired with "From the Business" (household `H4800`), or for
money going into the business, owner's equity (`3000`) paired with "Into
the Business" (`H6800`). Tax figures read only the business side.

**No closing entries.** Nothing is posted at year end. Retained earnings on
a balance sheet are computed: all income minus all expenses dated before
the current fiscal year started (`settings` key `fiscal_year_start_month`,
1 = January when missing), plus anything posted directly to a
`retained_earnings` account.

## Tables

Grouped by what they're for. The version in brackets is the schema step
that added the table.

### Book settings

**`settings`** [v1]: one `key`/`value` text pair per row.

| Key | Meaning |
|---|---|
| `book.sides` | `business`, `household`, or `business,household`. Missing means business only (books from before v5). |
| `lock_date` | `YYYY-MM-DD`. Entries on or before it can't be posted or voided (books closed for that period). |
| `fiscal_year_start_month` | 1–12. Where retained earnings split from this year's income. Missing means 1. |
| `bill_suggestions.dismissed` | JSON array of normalized payee texts the user said aren't bills. |
| `pref.company.name`, `pref.company.email`, `pref.company.phone`, `pref.company.address` | Shown on invoices and reports. |
| `pref.invoice.next_number` | Next invoice number to try. Missing means 1001. |
| `pref.invoice.footer`, `pref.invoice.default_terms_days`, `pref.invoice.default_tax_bp` | Invoice defaults. |
| `pref.tax.profile` | JSON: the owner's tax situation for estimates. Money fields are cents, `state_rate_bp` is basis points, `method` is `safe_harbor` or `current_year`. |
| `pref.cloud.folder`, `pref.cloud.git` | Where encrypted off-machine copies go: a synced folder, a Git repository. |
| `pref.cloud.auto`, `pref.cloud.keep`, `pref.cloud.last`, `pref.cloud.last_error` | `1` for a daily copy, how many copies to keep (10 when missing), when the last one was made (RFC 3339), and why the last one failed. |
| `pref.plaid.user_id` | A random id Tallybook gives Plaid for this book. |

### Accounts and the journal

**`accounts`** [v1, v5]: the chart of accounts.

| Column | Meaning |
|---|---|
| `code` | Chart number, unique: `1000`–`1999` assets, `2xxx` liabilities, `3xxx` equity, `4xxx` income, `5xxx` cost of goods sold, `6xxx` expenses, `7xxx` other expenses. Household accounts start with `H` (`H6200`). Optional for user-made accounts. |
| `name` | What people call it. |
| `subtype` | See below. Decides the account's kind and its natural sign. |
| `parent_id` | For sub-accounts; same kind and side as the parent. |
| `archived` | Hidden, keeps its history, takes no new entries. Only zero-balance accounts can be archived. |
| `scope` | `business` or `household`: which side of the book. |
| `flex` | Household spending categories only: `fixed` (rent), `usage` (power, groceries) or `lifestyle` (dining out). |

| `subtype` | Kind | Natural sign |
|---|---|---|
| `bank`, `accounts_receivable`, `other_current_asset`, `fixed_asset` | asset | debit (+) |
| `credit_card`, `accounts_payable`, `other_current_liability`, `long_term_liability` | liability | credit (−) |
| `equity`, `retained_earnings` | equity | credit (−) |
| `income`, `other_income` | income | credit (−) |
| `cost_of_goods_sold`, `expense`, `other_expense` | expense | debit (+) |

Some codes are found by convention and never change: `1100` Accounts
Receivable, `1200` Undeposited Funds, `2100` Accounts Payable, `2200` Sales
Tax Payable, `3000` Owner's Equity, `3050` Opening Balance Equity, `3100`
Owner's Draw, `3900` Retained Earnings, `4999` and `6999` uncategorized
income and expense; on the household side `H3050` opening balances, `H4800`
From the Business, `H6800` Into the Business, `H4999` and `H6999`
uncategorized.

**`journal_entries`** [v1, v4, v6, v8]: one row per transaction.

| Column | Meaning |
|---|---|
| `date` | The day the money moved (accounting date). |
| `memo` | Description. |
| `source` | What made it: `manual`, `opening_balance`, `bank_feed`, `invoice`, `bill`, `payment`, `tax_payment`, `void`, `ledger_import`. |
| `reverses_id` | On a void: the entry it cancels. |
| `voided_by_id` | On a voided entry: the void that cancels it. |
| `payee_id` | The customer or vendor (`contacts`), if known. Can be set after posting. |
| `person_id` | Household: who the money was for (`people`). Can be set after posting. |
| `goal_id` | Household: the savings goal this moved money toward (`goals`). |

**`journal_lines`** [v1]: the money.

| Column | Meaning |
|---|---|
| `entry_id`, `account_id` | Which entry, which account. |
| `amount` | Cents, **positive debit, negative credit**, never 0. |
| `memo` | Optional line description. |
| `cleared` | `uncleared`, `cleared` (seen on a statement or bank feed), or `reconciled` (locked into a completed reconciliation). |
| `reconciliation_id` | The reconciliation that ticked this line off. |

**`reconciliations`** [v1]: matching an account to a bank or card
statement. `statement_balance` is in the account's natural sign; `status` is
`in_progress` (at most one per account) or `completed`.

### Customers, vendors, and their documents

**`contacts`** [v3, v4]: `kind` is `customer`, `vendor` or `both`. For
1099 tracking: `is_1099`, `w9_on_file`, `legal_name`, `tin_last4`.

**`invoices`** and **`invoice_lines`** [v3]: money owed to the business.
`status` is `draft` (no journal entry yet, can be deleted), `open` (posted)
or `void`. `subtotal`, `tax` and `total` are cents; `tax_rate_bp` applies to
lines marked `taxable`. A line's `amount` is `quantity_milli × unit_price ÷
1000`, rounded. Posting debits Accounts Receivable (`1100`) for the total
and credits each line's income account, plus Sales Tax Payable (`2200`).

**`bills`** and **`bill_lines`** [v3]: money the business owes. `status`
is `open` or `void`. Posting debits each line's expense account and credits
Accounts Payable (`2100`).

**`payments`** [v3] and **`payment_applications`** [v3]: money received
(`kind = 'received'`) or paid (`made`). `account_id` is where the money went
or came from. A received payment debits that account and credits
Accounts Receivable; a payment made debits Accounts Payable and credits it.
Each application puts part of a payment against one invoice or one bill.
An invoice's balance due is its `total` minus applications from payments
whose `voided` is 0.

### Bank feeds

**`bank_connections`** [v2]: one per login at a bank or aggregator.
`provider` is `plaid`, `simplefin` or `file` (imported CSV/OFX).
`secret_ref` is the *name* of the keychain entry with the credentials
(`plaid:<uuid>` or `simplefin:<uuid>`), never the credentials. `cursor` is
Plaid's sync position. `status`: `ok`, `needs_relink`, `error`,
`disconnected`.

**`bank_links`** [v2]: a bank's account wired to a ledger account.
`bank_balance` is the bank's last reported balance, natural sign. Feed
transactions before `start_date` are ignored because the opening balance
already covers them.

**`bank_transactions`** [v2]: what the bank reported, waiting for review
or already filed. `amount` is **inflow-positive** (money into the account is
positive), which is also the debit-positive sign of the matching journal
line. `external_id` is the bank's id, unique per account, so re-importing
doesn't duplicate. `normalized` is the description lowercased with digits
removed, used to recognize a payee across months. `status`:

| `status` | Meaning |
|---|---|
| `new` | Waiting in Review. |
| `posted` | Turned into a new journal entry (`entry_id`, `line_id`), maybe by a rule (`rule_id`). |
| `matched` | Recognized as a line already in the books (`entry_id`, `line_id`). |
| `excluded` | The user said to ignore it (duplicate, personal, not real). |
| `removed` | The bank withdrew it (a pending charge that never posted). |

**`rules`** [v2, v6]: "when a description `op`s `pattern`, file it to
`target_account_id`". `op` is `contains`, `equals` or `starts_with`
(case-insensitive); `direction` is `any`, `inflow` or `outflow`;
`amount_min`/`amount_max` bound the absolute amount in cents. The lowest
`priority` (then lowest `id`) wins. `auto_post` = 1 files matches without
asking.

### Taxes (business side)

**`account_tax_lines`** [v4]: which IRS Schedule C line each business
income or expense account feeds, once the user has confirmed it. `line` is
the line number and name in snake case: `l1_gross_receipts`, `l2_returns`,
`l4_cost_of_goods`, `l6_other_income`, `l8_advertising`, `l9_car_truck`,
`l10_commissions`, `l11_contract_labor`, `l13_depreciation`,
`l14_employee_benefits`, `l15_insurance`, `l16a_mortgage_interest`,
`l16b_other_interest`, `l17_legal`, `l18_office`, `l19_pension`,
`l20a_rent_vehicles`, `l20b_rent_other`, `l21_repairs`, `l22_supplies`,
`l23_taxes`, `l24a_travel`, `l24b_meals`, `l25_utilities`, `l26_wages`,
`l27a_other`, or `not_deductible`. An account with no row hasn't been
confirmed.

**`tax_payments`** [v4]: estimated income tax paid (federal Form 1040-ES,
or a state's equivalent), by `tax_year`, `quarter` 1–4 and `jurisdiction`
(`federal` or `state`). Each posts an owner's draw (`3100`), because income
tax is the owner's, not a business expense.

### Household

**`people`** [v6]: who money was for. `color` names a UI color.

**`budget_months`** and **`budgets`** [v6]: the monthly plan. A month
without a `budget_months` row uses the plan of the latest earlier month that
has one. Each `budgets` row is one line for a person *or* a category,
never both. `direction` is `ceiling` (spend up to `amount`) or `floor` (put
in at least `amount`); `tolerance_pct` is the half-width of the "on plan"
band (10 means a $400 line is on plan from $360 to $440). `available` is
what the household said it had to work with that month.

**`recurring_bills`** and **`recurring_bill_marks`** [v7, v10]: reminders
for bills that come around (rent, phone, insurance). They're not
accounts-payable bills; nothing is owed in the books until a payment
posts. `cadence` is JSON: `{"every":"month","day":11}`,
`{"every":"months","months":6,"day":1}` or `{"every":"week"}`, counted from
`start_date`; a due day past the month's end means the last day. `amount`
0 means unknown or varying; `varies` = 1 means a usage bill whose amount
moves around its typical amount. A mark records what the user said about
one due date (`paid` or `skipped`).

**`goals`** [v8]: savings goals. Money for a goal sits in a real account
(`account_id`). Progress is `starting` plus the goal account's lines in
live entries tagged with that `goal_id`. `target_date` NULL means
"someday".

### Imports

**`import_links`** [v9]: what was brought in from another app (`source`,
e.g. `ledger`), so importing again adds only what's new. `kind` says which
table `local_id` points into: `entry`, `person`, `goal`, `bill` (a
recurring bill), `sector` (an account), `store` (the rule made from a saved
payee), `money`/`savings` (the account money moved through), or `month`
(`local_id` 0). The full list is at the top of
`crates/tally-store/src/ledger_import.rs`.

## Reports by hand

These run in the `sqlite3` shell against a plain book (or an unlocked
encrypted one). Replace the dates; `printf('%.2f', x / 100.0)` shows cents
as dollars for reading. Add `AND a.scope = 'business'` (or `'household'`)
to any of them for one side of the book.

**Trial balance** as of a date. Total debits equal total credits, always:

```sql
SELECT a.code, a.name,
       CASE WHEN SUM(l.amount) > 0 THEN printf('%.2f',  SUM(l.amount) / 100.0) END AS debit,
       CASE WHEN SUM(l.amount) < 0 THEN printf('%.2f', -SUM(l.amount) / 100.0) END AS credit
FROM journal_lines l
JOIN journal_entries e ON e.id = l.entry_id
JOIN accounts a ON a.id = l.account_id
WHERE e.date <= '2026-12-31'
GROUP BY a.id
HAVING SUM(l.amount) <> 0
ORDER BY a.code IS NULL, a.code, a.name;
```

**Profit and loss** for a period, income and expenses in natural sign:

```sql
SELECT a.subtype, a.code, a.name,
       printf('%.2f', CASE WHEN a.subtype IN ('income', 'other_income')
                           THEN -SUM(l.amount) ELSE SUM(l.amount) END / 100.0) AS amount
FROM journal_lines l
JOIN journal_entries e ON e.id = l.entry_id
JOIN accounts a ON a.id = l.account_id
WHERE e.date BETWEEN '2026-01-01' AND '2026-12-31'
  AND a.subtype IN ('income', 'other_income', 'cost_of_goods_sold', 'expense', 'other_expense')
GROUP BY a.id
ORDER BY a.code;

-- Net income: minus the sum of every income and expense line.
SELECT printf('%.2f', -SUM(l.amount) / 100.0) AS net_income
FROM journal_lines l
JOIN journal_entries e ON e.id = l.entry_id
JOIN accounts a ON a.id = l.account_id
WHERE e.date BETWEEN '2026-01-01' AND '2026-12-31'
  AND a.subtype IN ('income', 'other_income', 'cost_of_goods_sold', 'expense', 'other_expense');
```

**Balance sheet** as of a date. Assets = liabilities + equity + retained
earnings + this fiscal year's net income, where retained earnings is the
net income query run over every date before the fiscal year began:

```sql
SELECT CASE WHEN a.subtype IN ('bank', 'accounts_receivable', 'other_current_asset', 'fixed_asset') THEN 'asset'
            WHEN a.subtype IN ('credit_card', 'accounts_payable', 'other_current_liability', 'long_term_liability') THEN 'liability'
            ELSE 'equity' END AS section,
       a.code, a.name,
       printf('%.2f', CASE WHEN a.subtype IN ('bank', 'accounts_receivable', 'other_current_asset', 'fixed_asset')
                           THEN SUM(l.amount) ELSE -SUM(l.amount) END / 100.0) AS amount
FROM journal_lines l
JOIN journal_entries e ON e.id = l.entry_id
JOIN accounts a ON a.id = l.account_id
WHERE e.date <= '2026-12-31'
  AND a.subtype NOT IN ('income', 'other_income', 'cost_of_goods_sold', 'expense', 'other_expense')
GROUP BY a.id
HAVING SUM(l.amount) <> 0
ORDER BY section, a.code;
```

**An account's register** with a running balance (natural sign for an
asset; negate `l.amount` for a liability or card):

```sql
SELECT e.date, e.id AS entry, e.memo,
       printf('%.2f', l.amount / 100.0) AS amount,
       printf('%.2f', SUM(l.amount) OVER (ORDER BY e.date, e.id, l.id) / 100.0) AS balance,
       CASE WHEN e.voided_by_id IS NOT NULL OR e.reverses_id IS NOT NULL THEN 'void' ELSE '' END AS note
FROM journal_lines l
JOIN journal_entries e ON e.id = l.entry_id
WHERE l.account_id = (SELECT id FROM accounts WHERE code = '1000')
ORDER BY e.date, e.id, l.id;
```

**Unpaid invoices:**

```sql
SELECT number, customer, due_date, printf('%.2f', due / 100.0) AS balance_due
FROM (SELECT i.number, c.name AS customer, i.due_date,
             i.total - IFNULL((SELECT SUM(pa.amount)
                               FROM payment_applications pa
                               JOIN payments p ON p.id = pa.payment_id
                               WHERE pa.invoice_id = i.id AND p.voided = 0), 0) AS due
      FROM invoices i JOIN contacts c ON c.id = i.customer_id
      WHERE i.status = 'open')
WHERE due > 0
ORDER BY due_date;
```

**Household spending by category for a month:**

```sql
SELECT a.code, a.name, printf('%.2f', SUM(l.amount) / 100.0) AS spent
FROM journal_lines l
JOIN journal_entries e ON e.id = l.entry_id
JOIN accounts a ON a.id = l.account_id
WHERE a.scope = 'household' AND a.subtype IN ('expense', 'other_expense')
  AND substr(e.date, 1, 7) = '2026-10'
GROUP BY a.id
ORDER BY SUM(l.amount) DESC;
```

## Files next to the book

None of these are needed to read a book; they're conveniences.

- **`Backups/<book> YYYY-MM-DD HHMMSS.tallybook`**, beside the book: full
  copies from Settings → Back up now. A backup of an encrypted book is
  encrypted with the same key.
- **`Tax <year>/`**, beside the book: the accountant package (Taxes → For
  your accountant). Its `README.txt` lists every file. The `.csv` files are
  UTF-8, comma-separated with a header row and RFC 4180 quoting; amounts are
  dollars with two decimals and a leading `-` when negative; dates are
  `YYYY-MM-DD`. The `.pdf` files are the same figures for reading.
- **Encrypted cloud copies** go to `<chosen folder>/Tallybook/<book>
  YYYY-MM-DD HHMMSS.tallybook`, or are committed as `<book>.tallybook` to a
  Git repository. Only encrypted books are ever copied off the computer.

## Files in the app's settings folder

Tallybook keeps a few small JSON files, all UTF-8, in its settings folder:
`~/Library/Application Support/app.tallybook.desktop` on macOS,
`%APPDATA%\app.tallybook.desktop` on Windows, and
`~/.config/app.tallybook.desktop` (or `$XDG_CONFIG_HOME`) on Linux.
Development builds use a `dev` folder inside it. Losing any of them loses
no bookkeeping. Each is written whole (a temporary file renamed over the old
one), and on macOS and Linux readable only by the user (mode 0600, folder
0700).

- **`recent.json`**: the books on the welcome screen, newest first, at most
  ten: `[{"path": "...", "name": "...", "last_opened": "<RFC 3339>", "encrypted": false}]`.
  `encrypted` is rechecked from the file's header each time the list is read.
- **`keys.json`**: a JSON array of uuid strings. Each names a keychain entry
  `book-key:<uuid>` holding one book key (see
  [Where the key is](#where-the-key-is)). It lists ids, never keys.
- **`license.json`**: what this computer's license covers: `tier` (`free`,
  `business` or `tax`), the license `key` and Lemon Squeezy `instance_id`
  it was activated with, `email`, `activated_on` (`YYYY-MM-DD`),
  `tax_years` (calendar years tax prep is unlocked for) and `earlier` (keys
  activated here before the current one). Each key itself is in the keychain
  as `license-key:<instance_id>`, and `key` holds only its last four
  characters; files written by earlier versions hold whole keys, which still
  work. A license never locks anyone out
  of reading a book.
