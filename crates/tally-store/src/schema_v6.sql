-- Tallybook schema v6: people and monthly household budgets.

-- Who the money was for. A person is "who"; the category is "what". Only
-- household money is budgeted by person; untagged spending is shared.
CREATE TABLE people (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    color      TEXT NOT NULL DEFAULT 'slate',
    sort_order INTEGER NOT NULL DEFAULT 0,
    archived   INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Metadata like payee_id, so it may be set after posting.
ALTER TABLE journal_entries ADD COLUMN person_id INTEGER REFERENCES people(id);
CREATE INDEX journal_entries_person ON journal_entries(person_id);
ALTER TABLE rules ADD COLUMN person_id INTEGER REFERENCES people(id);

-- A month with its own plan. Months without a row inherit the plan of the
-- latest earlier month that has one: the plan carries forward on the 1st,
-- the spending never does.
CREATE TABLE budget_months (
    month     TEXT PRIMARY KEY CHECK (month GLOB '[0-9][0-9][0-9][0-9]-[0-1][0-9]'),
    -- What there is to work with this month, if she has said.
    available INTEGER CHECK (available IS NULL OR available >= 0)
);

-- One line of a month's plan, for a person or for a category.
CREATE TABLE budgets (
    id            INTEGER PRIMARY KEY,
    month         TEXT NOT NULL REFERENCES budget_months(month),
    person_id     INTEGER REFERENCES people(id),
    account_id    INTEGER REFERENCES accounts(id),
    amount        INTEGER NOT NULL CHECK (amount >= 0),
    direction     TEXT NOT NULL DEFAULT 'ceiling' CHECK (direction IN ('ceiling', 'floor')),
    tolerance_pct INTEGER NOT NULL DEFAULT 10 CHECK (tolerance_pct BETWEEN 0 AND 100),
    CHECK ((person_id IS NULL) <> (account_id IS NULL))
);
CREATE UNIQUE INDEX budgets_person ON budgets(month, person_id) WHERE person_id IS NOT NULL;
CREATE UNIQUE INDEX budgets_account ON budgets(month, account_id) WHERE account_id IS NOT NULL;
