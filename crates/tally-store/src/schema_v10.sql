-- Tallybook schema v10: bills whose amount moves around.

-- A usage bill (power, water) keeps its typical amount, so a payment only
-- ticks it off when it's somewhere near that, not whatever it happens to be.
-- An amount of 0 still means "not known yet".
ALTER TABLE recurring_bills ADD COLUMN varies INTEGER NOT NULL DEFAULT 0 CHECK (varies IN (0, 1));
