-- Tallybook schema v5: household books.
-- A book keeps a business, a household, or both. Every account belongs to one
-- side, and each journal entry balances within each side on its own; money
-- that crosses sides gets owner's-draw lines added at posting time.
-- Accounts from before v5 are all business, hence the default.

ALTER TABLE accounts ADD COLUMN scope TEXT NOT NULL DEFAULT 'business'
    CHECK (scope IN ('business', 'household'));
-- How far a household spending category can bend (opportunity gaps):
-- fixed (rent), usage (power, groceries), lifestyle (dining out). Null for
-- every account that isn't a household spending category.
ALTER TABLE accounts ADD COLUMN flex TEXT CHECK (flex IS NULL OR flex IN ('fixed', 'usage', 'lifestyle'));
