-- Tallybook schema v8: household savings goals.
-- A goal is money kept in a real account (savings, usually), so it's never
-- a parallel ledger: putting money toward it is an ordinary transfer into
-- that account, tagged with the goal. Goals accumulate across months
-- instead of resetting on the 1st.

CREATE TABLE goals (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    target      INTEGER NOT NULL CHECK (target > 0),
    -- Null is "someday": no deadline to feel behind on.
    target_date TEXT,
    -- Where the money is kept.
    account_id  INTEGER NOT NULL REFERENCES accounts(id),
    -- Already saved before the goal was set up here.
    starting    INTEGER NOT NULL DEFAULT 0 CHECK (starting >= 0),
    note        TEXT NOT NULL DEFAULT '',
    achieved_at TEXT,
    archived    INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

ALTER TABLE journal_entries ADD COLUMN goal_id INTEGER REFERENCES goals(id);
CREATE INDEX journal_entries_goal ON journal_entries(goal_id);
