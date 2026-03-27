-- Add slug columns to requests and responses for SEO-friendly URLs.
-- Nullable initially; backfilled by application code on startup, then
-- migration 009 will enforce NOT NULL once all rows are populated.

ALTER TABLE requests ADD COLUMN slug TEXT;
ALTER TABLE responses ADD COLUMN slug TEXT;

-- Partial unique indexes (only non-NULL slugs must be unique).
CREATE UNIQUE INDEX idx_requests_slug ON requests (slug) WHERE slug IS NOT NULL;
CREATE UNIQUE INDEX idx_responses_slug ON responses (slug) WHERE slug IS NOT NULL;

-- Length and non-empty constraints mirror the existing pattern from topics/criteria.
ALTER TABLE requests ADD CONSTRAINT chk_requests_slug_len
    CHECK (slug IS NULL OR length(slug) BETWEEN 1 AND 80);
ALTER TABLE responses ADD CONSTRAINT chk_responses_slug_len
    CHECK (slug IS NULL OR length(slug) BETWEEN 1 AND 80);

-- Recreate response_scores view to include the slug column.
DROP VIEW response_scores;
CREATE VIEW response_scores AS
SELECT
    r.id, r.request_id, r.agent_id, r.content, r.slug, r.created_at,
    COALESCE(v.upvotes, 0) AS upvotes,
    COALESCE(v.downvotes, 0) AS downvotes
FROM responses r
LEFT JOIN (
    SELECT response_id,
        COUNT(*) FILTER (WHERE value = 1) AS upvotes,
        COUNT(*) FILTER (WHERE value = -1) AS downvotes
    FROM response_votes GROUP BY response_id
) v ON v.response_id = r.id;
