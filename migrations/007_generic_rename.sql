-- ============================================================
-- Migration 007: Generic rename — remove domain-specific terminology
-- and introduce dynamic criteria for scoring dimensions
-- ============================================================

-- ----------------------------------------------------------------
-- 1. Drop dependent views
-- ----------------------------------------------------------------
DROP VIEW IF EXISTS bot_stats;
DROP VIEW IF EXISTS kural_scores;

-- ----------------------------------------------------------------
-- 2. Create criteria table (replaces hardcoded score_type enum)
-- ----------------------------------------------------------------
CREATE TABLE criteria (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        VARCHAR(100) NOT NULL,
    slug        VARCHAR(60) NOT NULL UNIQUE,
    description VARCHAR(500),
    weight      REAL NOT NULL DEFAULT 0.0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chk_criteria_name_nonempty CHECK (length(trim(name)) > 0),
    CONSTRAINT chk_criteria_name_len CHECK (length(name) <= 100),
    CONSTRAINT chk_criteria_slug_nonempty CHECK (length(trim(slug)) > 0),
    CONSTRAINT chk_criteria_slug_len CHECK (length(slug) <= 60),
    CONSTRAINT chk_criteria_description_nonempty CHECK (description IS NULL OR length(trim(description)) > 0),
    CONSTRAINT chk_criteria_description_len CHECK (description IS NULL OR length(description) <= 500),
    CONSTRAINT chk_criteria_weight_range CHECK (weight >= 0.0 AND weight <= 1.0)
);

CREATE TRIGGER trg_criteria_updated_at
    BEFORE UPDATE ON criteria
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Seed with initial criteria (matching previous meaning/prosody dimensions)
INSERT INTO criteria (name, slug, description, weight) VALUES
    ('Meaning', 'meaning', 'Semantic alignment with the prompt', 0.33),
    ('Prosody', 'prosody', 'Structural and rhythmic quality', 0.33);

-- ----------------------------------------------------------------
-- 3. Rename tables
-- ----------------------------------------------------------------
ALTER TABLE bots RENAME TO agents;
ALTER TABLE kurals RENAME TO responses;
ALTER TABLE kural_votes RENAME TO response_votes;
ALTER TABLE judge_scores RENAME TO evaluations;

-- ----------------------------------------------------------------
-- 4. Migrate evaluations to use criterion_id (before dropping score_type)
-- ----------------------------------------------------------------
ALTER TABLE evaluations ADD COLUMN criterion_id UUID;

UPDATE evaluations
SET criterion_id = c.id
FROM criteria c
WHERE c.slug = evaluations.score_type::text;

ALTER TABLE evaluations ALTER COLUMN criterion_id SET NOT NULL;
ALTER TABLE evaluations ADD CONSTRAINT evaluations_criterion_id_fkey
    FOREIGN KEY (criterion_id) REFERENCES criteria(id);

-- Drop old unique constraint and score_type column
ALTER TABLE evaluations DROP CONSTRAINT judge_scores_kural_id_bot_id_score_type_key;
ALTER TABLE evaluations DROP COLUMN score_type;

-- ----------------------------------------------------------------
-- 5. Rename columns
-- ----------------------------------------------------------------

-- agents (was bots)
-- bot_type column rename happens after enum migration below

-- requests
ALTER TABLE requests RENAME COLUMN meaning TO prompt;

-- responses (was kurals)
ALTER TABLE responses RENAME COLUMN raw_text TO content;
ALTER TABLE responses RENAME COLUMN bot_id TO agent_id;

-- response_votes (was kural_votes)
ALTER TABLE response_votes RENAME COLUMN kural_id TO response_id;

-- evaluations (was judge_scores) — kural_id and bot_id
ALTER TABLE evaluations RENAME COLUMN kural_id TO response_id;
ALTER TABLE evaluations RENAME COLUMN bot_id TO agent_id;

-- comments
ALTER TABLE comments RENAME COLUMN kural_id TO response_id;

-- ----------------------------------------------------------------
-- 6. Replace bot_type enum with agent_role
-- ----------------------------------------------------------------
-- First merge prosody_judge into meaning_judge (both become evaluator)
UPDATE agents SET bot_type = 'meaning_judge' WHERE bot_type = 'prosody_judge';

-- Create new enum and migrate
CREATE TYPE agent_role AS ENUM ('creator', 'evaluator');
ALTER TABLE agents ALTER COLUMN bot_type TYPE agent_role
    USING CASE
        WHEN bot_type::text = 'poet' THEN 'creator'::agent_role
        ELSE 'evaluator'::agent_role
    END;
ALTER TABLE agents RENAME COLUMN bot_type TO agent_role;
DROP TYPE bot_type;

-- ----------------------------------------------------------------
-- 7. Drop score_type enum (column already dropped)
-- ----------------------------------------------------------------
DROP TYPE score_type;

-- ----------------------------------------------------------------
-- 8. Drop composite_score SQL function (moving to Rust)
-- ----------------------------------------------------------------
DROP FUNCTION IF EXISTS composite_score;

-- ----------------------------------------------------------------
-- 9. New unique constraint on evaluations
-- ----------------------------------------------------------------
ALTER TABLE evaluations ADD CONSTRAINT evaluations_response_agent_criterion_key
    UNIQUE (response_id, agent_id, criterion_id);

-- ----------------------------------------------------------------
-- 10. Rename indexes
-- ----------------------------------------------------------------
ALTER INDEX idx_bots_owner RENAME TO idx_agents_owner;
ALTER INDEX idx_bots_type_active RENAME TO idx_agents_role_active;
ALTER INDEX idx_bots_owner_name RENAME TO idx_agents_owner_name;
ALTER INDEX idx_kurals_request RENAME TO idx_responses_request;
ALTER INDEX idx_kurals_bot RENAME TO idx_responses_agent;
ALTER INDEX idx_kurals_created RENAME TO idx_responses_created;
ALTER INDEX idx_kural_votes_kural_value RENAME TO idx_response_votes_response_value;
ALTER INDEX idx_kural_votes_user RENAME TO idx_response_votes_user;
ALTER INDEX idx_judge_scores_kural_type RENAME TO idx_evaluations_response_criterion;
ALTER INDEX idx_comments_kural RENAME TO idx_comments_response;

-- ----------------------------------------------------------------
-- 11. Rename foreign key constraints
-- ----------------------------------------------------------------

-- agents (was bots)
ALTER TABLE agents RENAME CONSTRAINT bots_owner_id_fkey TO agents_owner_id_fkey;

-- responses (was kurals)
ALTER TABLE responses RENAME CONSTRAINT kurals_request_id_fkey TO responses_request_id_fkey;
ALTER TABLE responses RENAME CONSTRAINT kurals_bot_id_fkey TO responses_agent_id_fkey;

-- evaluations (was judge_scores)
ALTER TABLE evaluations RENAME CONSTRAINT judge_scores_bot_id_fkey TO evaluations_agent_id_fkey;

-- response_votes (was kural_votes)
ALTER TABLE response_votes RENAME CONSTRAINT kural_votes_user_id_fkey TO response_votes_user_id_fkey;

-- ----------------------------------------------------------------
-- 12. Rename CHECK constraints
-- ----------------------------------------------------------------

-- responses (was kurals)
ALTER TABLE responses RENAME CONSTRAINT chk_kurals_raw_text_len TO chk_responses_content_len;
ALTER TABLE responses RENAME CONSTRAINT chk_kurals_raw_text_nonempty TO chk_responses_content_nonempty;

-- requests
ALTER TABLE requests RENAME CONSTRAINT chk_requests_meaning_len TO chk_requests_prompt_len;
ALTER TABLE requests RENAME CONSTRAINT chk_requests_meaning_nonempty TO chk_requests_prompt_nonempty;

-- evaluations (was judge_scores)
ALTER TABLE evaluations RENAME CONSTRAINT chk_judge_scores_reasoning_len TO chk_evaluations_reasoning_len;
ALTER TABLE evaluations RENAME CONSTRAINT chk_judge_scores_reasoning_nonempty TO chk_evaluations_reasoning_nonempty;

-- agents (was bots)
ALTER TABLE agents RENAME CONSTRAINT chk_bots_name_len TO chk_agents_name_len;
ALTER TABLE agents RENAME CONSTRAINT chk_bots_name_nonempty TO chk_agents_name_nonempty;
ALTER TABLE agents RENAME CONSTRAINT chk_bots_description_len TO chk_agents_description_len;
ALTER TABLE agents RENAME CONSTRAINT chk_bots_description_nonempty TO chk_agents_description_nonempty;
ALTER TABLE agents RENAME CONSTRAINT chk_bots_model_name_len TO chk_agents_model_name_len;
ALTER TABLE agents RENAME CONSTRAINT chk_bots_model_name_nonempty TO chk_agents_model_name_nonempty;
ALTER TABLE agents RENAME CONSTRAINT chk_bots_model_version_len TO chk_agents_model_version_len;
ALTER TABLE agents RENAME CONSTRAINT chk_bots_model_version_nonempty TO chk_agents_model_version_nonempty;

-- comments: rename target check (column was renamed from kural_id to response_id)
ALTER TABLE comments RENAME CONSTRAINT chk_comments_target TO chk_comments_target_entity;

-- ----------------------------------------------------------------
-- 13. Rename triggers
-- ----------------------------------------------------------------
ALTER TRIGGER trg_bots_updated_at ON agents RENAME TO trg_agents_updated_at;
ALTER TRIGGER trg_judge_scores_updated_at ON evaluations RENAME TO trg_evaluations_updated_at;

-- ----------------------------------------------------------------
-- 14. Update config table
-- ----------------------------------------------------------------
-- Migrate score_weights to vote_weight (criterion weights are in criteria table)
UPDATE config SET value = jsonb_build_object('vote', COALESCE((value->>'community')::real, 0.34))
WHERE key = 'score_weights';
UPDATE config SET key = 'vote_weight' WHERE key = 'score_weights';

-- Update config known-keys constraint
ALTER TABLE config DROP CONSTRAINT chk_config_known_keys;
ALTER TABLE config ADD CONSTRAINT chk_config_known_keys
    CHECK (key IN ('vote_weight'));

-- ----------------------------------------------------------------
-- 15. Recreate views with new names and schema
-- ----------------------------------------------------------------

-- response_scores: simplified, vote_score only (criteria averages computed at query time)
CREATE VIEW response_scores AS
SELECT
    r.id, r.request_id, r.agent_id, r.content, r.created_at,
    COALESCE(v.upvotes, 0) AS upvotes,
    COALESCE(v.downvotes, 0) AS downvotes,
    wilson_lower_bound(COALESCE(v.upvotes, 0), COALESCE(v.downvotes, 0)) AS vote_score
FROM responses r
LEFT JOIN (
    SELECT response_id,
        COUNT(*) FILTER (WHERE value = 1) AS upvotes,
        COUNT(*) FILTER (WHERE value = -1) AS downvotes
    FROM response_votes GROUP BY response_id
) v ON v.response_id = r.id;

-- agent_stats: simplified (no composite score, just counts)
CREATE VIEW agent_stats AS
SELECT
    a.id, a.owner_id, a.agent_role, a.name, a.description,
    a.model_name, a.model_version, a.is_active,
    a.created_at, a.updated_at,
    COALESCE(rs.response_count, 0) AS response_count
FROM agents a
LEFT JOIN (
    SELECT agent_id, COUNT(*) AS response_count
    FROM responses
    GROUP BY agent_id
) rs ON rs.agent_id = a.id;

-- ----------------------------------------------------------------
-- 16. Add index for evaluations by criterion
-- ----------------------------------------------------------------
CREATE INDEX idx_evaluations_criterion ON evaluations (criterion_id);
