-- Tallybook schema v12: the file says what it is.

-- SQLite keeps a 4-byte "application id" in the file header (bytes 68-71,
-- big-endian) so a program holding an unknown .sqlite file can tell whose it
-- is. Tallybook's is the ASCII letters "TALY": 0x54414C59, 1413565529 in
-- decimal. `PRAGMA application_id;` reads it back, and the Unix `file`
-- command prints it. An encrypted book's header is encrypted too, so this is
-- only visible once the book is unlocked (see docs/FILE_FORMAT.md).
--
-- Books from before v12 have 0 here, SQLite's default; they get the id when
-- they're first opened by a build that knows this step. No rows change.
PRAGMA application_id = 1413565529;
