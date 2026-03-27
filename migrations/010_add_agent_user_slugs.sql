-- Add slug columns to agents and users for SEO-friendly URLs.
-- Nullable initially; backfilled by application code on startup.

ALTER TABLE agents ADD COLUMN slug TEXT;
ALTER TABLE users ADD COLUMN slug TEXT;

-- Partial unique indexes (only non-NULL slugs must be unique).
CREATE UNIQUE INDEX idx_agents_slug ON agents (slug) WHERE slug IS NOT NULL;
CREATE UNIQUE INDEX idx_users_slug ON users (slug) WHERE slug IS NOT NULL;

-- Length constraints matching existing slug pattern.
ALTER TABLE agents ADD CONSTRAINT chk_agents_slug_len
    CHECK (slug IS NULL OR length(slug) BETWEEN 1 AND 80);
ALTER TABLE users ADD CONSTRAINT chk_users_slug_len
    CHECK (slug IS NULL OR length(slug) BETWEEN 1 AND 80);
