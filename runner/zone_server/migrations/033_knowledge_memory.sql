-- Memory the model writes for one person needs two things this table does not
-- have. A token a writer echoes to prove it read the row it is replacing:
-- updated_at will not do, because every statement in one transaction shares
-- NOW() and the column has no uniqueness, so two writes can carry the same
-- token and a lost update passes the check. And a sentence saying what an
-- entry is for, so a later turn can decide whether to read it without reading
-- all of them. Named version rather than revision: Document::revision on this
-- same table is already a content hash. Zero means no memory tool has ever
-- written the row, which is true of every entry that exists today, so a
-- NOT NULL column with a constant default is metadata only -- no scan, no
-- backfill. The unique index one name per person per kind rests on is built in
-- 034, outside this transaction: a build here would hold what these two
-- statements already hold until commit, and read the whole table to do it.
SET LOCAL lock_timeout = '5s';
ALTER TABLE knowledge_entries ADD COLUMN IF NOT EXISTS version BIGINT NOT NULL DEFAULT 0;
ALTER TABLE knowledge_entries ADD COLUMN IF NOT EXISTS description TEXT;
