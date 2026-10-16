-- Who laid a row. scher-core's EventRow has carried `laid_by` all along; this table
-- never stored it, so every consumer read authorship back as unknown. Nullable: rows
-- laid before this column existed stay honestly unattributed rather than guessed.
ALTER TABLE stacks.event ADD COLUMN IF NOT EXISTS laid_by text NULL;
