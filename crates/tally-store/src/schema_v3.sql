-- Tallybook schema v3: customers, vendors, invoices, bills, payments.
-- Documents are the human-facing record; each non-draft document owns exactly
-- one journal entry, which is what the reports read.

CREATE TABLE contacts (
    id         INTEGER PRIMARY KEY,
    kind       TEXT NOT NULL CHECK (kind IN ('customer', 'vendor', 'both')),
    name       TEXT NOT NULL,
    company    TEXT,
    email      TEXT,
    phone      TEXT,
    address    TEXT,
    notes      TEXT,
    archived   INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE invoices (
    id          INTEGER PRIMARY KEY,
    number      TEXT NOT NULL UNIQUE,
    customer_id INTEGER NOT NULL REFERENCES contacts(id),
    issue_date  TEXT NOT NULL,
    due_date    TEXT NOT NULL,
    memo        TEXT NOT NULL DEFAULT '',
    tax_rate_bp INTEGER NOT NULL DEFAULT 0 CHECK (tax_rate_bp BETWEEN 0 AND 10000),
    status      TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'open', 'void')),
    subtotal    INTEGER NOT NULL DEFAULT 0,
    tax         INTEGER NOT NULL DEFAULT 0,
    total       INTEGER NOT NULL DEFAULT 0,
    entry_id    INTEGER REFERENCES journal_entries(id),
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX invoices_customer ON invoices(customer_id);

CREATE TABLE invoice_lines (
    id             INTEGER PRIMARY KEY,
    invoice_id     INTEGER NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    position       INTEGER NOT NULL,
    description    TEXT NOT NULL,
    quantity_milli INTEGER NOT NULL CHECK (quantity_milli > 0),
    unit_price     INTEGER NOT NULL,
    amount         INTEGER NOT NULL,
    account_id     INTEGER NOT NULL REFERENCES accounts(id),
    taxable        INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX invoice_lines_invoice ON invoice_lines(invoice_id);

CREATE TABLE bills (
    id         INTEGER PRIMARY KEY,
    vendor_id  INTEGER NOT NULL REFERENCES contacts(id),
    number     TEXT NOT NULL DEFAULT '',
    issue_date TEXT NOT NULL,
    due_date   TEXT NOT NULL,
    memo       TEXT NOT NULL DEFAULT '',
    status     TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'void')),
    total      INTEGER NOT NULL,
    entry_id   INTEGER NOT NULL REFERENCES journal_entries(id),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX bills_vendor ON bills(vendor_id);

CREATE TABLE bill_lines (
    id          INTEGER PRIMARY KEY,
    bill_id     INTEGER NOT NULL REFERENCES bills(id),
    position    INTEGER NOT NULL,
    description TEXT NOT NULL,
    amount      INTEGER NOT NULL CHECK (amount > 0),
    account_id  INTEGER NOT NULL REFERENCES accounts(id)
);

CREATE TABLE payments (
    id         INTEGER PRIMARY KEY,
    kind       TEXT NOT NULL CHECK (kind IN ('received', 'made')),
    contact_id INTEGER NOT NULL REFERENCES contacts(id),
    date       TEXT NOT NULL,
    amount     INTEGER NOT NULL CHECK (amount > 0),
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    method     TEXT NOT NULL DEFAULT '',
    reference  TEXT NOT NULL DEFAULT '',
    memo       TEXT NOT NULL DEFAULT '',
    voided     INTEGER NOT NULL DEFAULT 0 CHECK (voided IN (0, 1)),
    entry_id   INTEGER NOT NULL REFERENCES journal_entries(id),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE payment_applications (
    id         INTEGER PRIMARY KEY,
    payment_id INTEGER NOT NULL REFERENCES payments(id),
    invoice_id INTEGER REFERENCES invoices(id),
    bill_id    INTEGER REFERENCES bills(id),
    amount     INTEGER NOT NULL CHECK (amount > 0),
    CHECK ((invoice_id IS NULL) <> (bill_id IS NULL))
);
CREATE INDEX payment_applications_invoice ON payment_applications(invoice_id);
CREATE INDEX payment_applications_bill ON payment_applications(bill_id);
