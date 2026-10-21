-- Tallybook schema v7: recurring household bills.

-- Household bills that come around again: rent on the 1st, the phone on the
-- 11th, car insurance every six months. Not accounts-payable bills: nothing
-- is owed in the books until it's paid; these are reminders that the bank
-- feed ticks off.
CREATE TABLE recurring_bills (
    id           INTEGER PRIMARY KEY,
    name         TEXT NOT NULL,
    -- 0 when it varies (utilities) or isn't known yet.
    amount       INTEGER NOT NULL DEFAULT 0 CHECK (amount >= 0),
    -- The spending category it's paid into, which is what a payment matches on.
    account_id   INTEGER NOT NULL REFERENCES accounts(id),
    paid_from_id INTEGER REFERENCES accounts(id),
    person_id    INTEGER REFERENCES people(id),
    -- tally_core::schedule::Cadence as JSON.
    cadence      TEXT NOT NULL,
    start_date   TEXT NOT NULL,
    auto_pay     INTEGER NOT NULL DEFAULT 0 CHECK (auto_pay IN (0, 1)),
    -- Text a payment's description must contain, e.g. "VERIZON". Optional.
    match_text   TEXT,
    active       INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

-- What the user said about one due date, overriding automatic matching.
CREATE TABLE recurring_bill_marks (
    bill_id  INTEGER NOT NULL REFERENCES recurring_bills(id),
    due_date TEXT NOT NULL,
    status   TEXT NOT NULL CHECK (status IN ('paid', 'skipped')),
    entry_id INTEGER REFERENCES journal_entries(id),
    PRIMARY KEY (bill_id, due_date)
);
