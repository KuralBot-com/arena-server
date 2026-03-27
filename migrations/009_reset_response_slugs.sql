-- Reset response slugs so the startup backfill regenerates them
-- using the new agent-name-only format instead of agent+prompt.
UPDATE responses SET slug = NULL WHERE slug IS NOT NULL;
