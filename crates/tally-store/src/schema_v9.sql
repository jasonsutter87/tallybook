-- Tallybook schema v9: what was brought in from another app, so bringing it
-- in again adds only what's new.

CREATE TABLE import_links (
    source      TEXT NOT NULL,      -- e.g. 'ledger'
    kind        TEXT NOT NULL,      -- 'entry', 'person', 'goal', ...
    external_id TEXT NOT NULL,
    local_id    INTEGER NOT NULL,
    PRIMARY KEY (source, kind, external_id)
);
