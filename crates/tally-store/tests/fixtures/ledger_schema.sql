-- The schema of Ledger after its sixth migration, copied
-- from src/lib/db.ts, for testing the importer against the real shape.
-- Do not tidy it: the CREATE-then-ALTER order is Ledger's migration history,
-- and the importer checks for the later columns before reading them, so
-- older Ledger files still import. Used by tests/ledger_import.rs.
--
-- Conventions in Ledger's data: ids are TEXT (any unique string); dates are
-- ISO 8601 text ('2026-08-01'); month ids are 'YYYY-MM'; amounts are signed
-- integer cents, negative for spending; booleans are INTEGER 0/1.

-- Core tables: months, people, per-person budgets, entries, bills.
-- allocated_cents is what there is to work with that month.
CREATE TABLE month (id TEXT PRIMARY KEY, allocated_cents INTEGER NOT NULL DEFAULT 0, opened_at TEXT NOT NULL, closed_at TEXT);
CREATE TABLE person (id TEXT PRIMARY KEY, name TEXT NOT NULL, color TEXT NOT NULL DEFAULT 'slate', sort_order INTEGER NOT NULL DEFAULT 0, archived_at TEXT);
-- One person's budget line for a month: a ceiling (limit) or a floor (minimum).
CREATE TABLE allocation (
  month_id TEXT NOT NULL REFERENCES month(id) ON DELETE CASCADE,
  person_id TEXT NOT NULL REFERENCES person(id) ON DELETE CASCADE,
  amount_cents INTEGER NOT NULL DEFAULT 0,
  direction TEXT NOT NULL DEFAULT 'ceiling' CHECK(direction IN ('ceiling','floor')),
  tolerance_pct INTEGER NOT NULL DEFAULT 10,
  PRIMARY KEY (month_id, person_id));
CREATE TABLE entry (
  id TEXT PRIMARY KEY, month_id TEXT NOT NULL REFERENCES month(id) ON DELETE CASCADE,
  person_id TEXT REFERENCES person(id) ON DELETE SET NULL, occurred_on TEXT NOT NULL,
  amount_cents INTEGER NOT NULL, note TEXT NOT NULL DEFAULT '',
  source TEXT NOT NULL DEFAULT 'manual' CHECK(source IN ('manual','recurring','import')),
  external_id TEXT, created_at TEXT NOT NULL);
-- Bills. day_of_week counts from Sunday = 0; day_of_month is 1-31.
CREATE TABLE recurring (
  id TEXT PRIMARY KEY, person_id TEXT NOT NULL REFERENCES person(id) ON DELETE CASCADE, label TEXT NOT NULL,
  amount_cents INTEGER NOT NULL, cadence TEXT NOT NULL CHECK(cadence IN ('weekly','monthly','every_n_months')),
  day_of_week INTEGER, day_of_month INTEGER, months_interval INTEGER, active INTEGER NOT NULL DEFAULT 1);
-- Added by later migrations. Sectors are categories; a store is a remembered payee
-- that fills in the sector (and person) by match_key.
CREATE TABLE sector (id TEXT PRIMARY KEY, name TEXT NOT NULL, sort_order INTEGER NOT NULL DEFAULT 0, archived_at TEXT);
CREATE TABLE store (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, sector_id TEXT REFERENCES sector(id) ON DELETE SET NULL,
  default_person_id TEXT REFERENCES person(id) ON DELETE SET NULL, match_key TEXT NOT NULL,
  use_count INTEGER NOT NULL DEFAULT 0, last_used_at TEXT, archived_at TEXT);
ALTER TABLE entry ADD COLUMN store_id TEXT REFERENCES store(id) ON DELETE SET NULL;
ALTER TABLE entry ADD COLUMN sector_id TEXT REFERENCES sector(id) ON DELETE SET NULL;
-- Goals, filled by entries tagged with goal_id.
CREATE TABLE goal (id TEXT PRIMARY KEY, name TEXT NOT NULL, target_cents INTEGER NOT NULL, target_date TEXT, note TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL, achieved_at TEXT, archived_at TEXT);
ALTER TABLE entry ADD COLUMN goal_id TEXT REFERENCES goal(id) ON DELETE SET NULL;
ALTER TABLE recurring ADD COLUMN sector_id TEXT REFERENCES sector(id) ON DELETE SET NULL;
ALTER TABLE recurring ADD COLUMN auto_debit INTEGER NOT NULL DEFAULT 0;
-- 'low' | 'medium' | 'high': how easily the spending can be cut. Tally reads
-- it as flex Fixed | Usage | Lifestyle.
ALTER TABLE sector ADD COLUMN changeability TEXT NOT NULL DEFAULT 'medium';
