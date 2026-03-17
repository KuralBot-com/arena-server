-- ============================================================
-- Migration 003: Schema quality improvements
-- ============================================================

-- 0. Drop bot_api_keys (API key management handled by AWS API Gateway)
DROP TABLE IF EXISTS bot_api_keys;

-- 1. Convert score_type from TEXT+CHECK to proper enum
CREATE TYPE score_type AS ENUM ('meaning', 'prosody');

-- Must drop views that depend on judge_scores before altering column
DROP VIEW IF EXISTS bot_stats;
DROP VIEW IF EXISTS kural_scores;

-- Drop the CHECK constraint, then convert column type
ALTER TABLE judge_scores DROP CONSTRAINT IF EXISTS judge_scores_score_type_check;
ALTER TABLE judge_scores ALTER COLUMN score_type TYPE score_type USING score_type::score_type;

-- 2. Trigger function for updated_at
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Add updated_at to judge_scores (previously overwriting created_at on upsert)
ALTER TABLE judge_scores ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- Attach triggers to all tables with updated_at
CREATE TRIGGER trg_users_updated_at BEFORE UPDATE ON users FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_bots_updated_at BEFORE UPDATE ON bots FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_requests_updated_at BEFORE UPDATE ON requests FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_config_updated_at BEFORE UPDATE ON config FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_judge_scores_updated_at BEFORE UPDATE ON judge_scores FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- 3. Unique constraint: one bot name per owner
CREATE UNIQUE INDEX idx_bots_owner_name ON bots (owner_id, name);

-- 4. Unique constraint: one account per email
CREATE UNIQUE INDEX idx_users_email ON users (email);

-- 5. Fix ON DELETE policies on vote tables (votes should cascade with user deletion)
ALTER TABLE request_votes DROP CONSTRAINT request_votes_user_id_fkey;
ALTER TABLE request_votes ADD CONSTRAINT request_votes_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

ALTER TABLE kural_votes DROP CONSTRAINT kural_votes_user_id_fkey;
ALTER TABLE kural_votes ADD CONSTRAINT kural_votes_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;

-- 6. Indexes for leaderboard user_id lookups
CREATE INDEX idx_request_votes_user ON request_votes (user_id);
CREATE INDEX idx_kural_votes_user ON kural_votes (user_id);

-- 7. Text length CHECK constraints (defense-in-depth, matching app validation)
ALTER TABLE users ADD CONSTRAINT chk_users_display_name_len CHECK (length(display_name) <= 100);
ALTER TABLE users ADD CONSTRAINT chk_users_email_len CHECK (length(email) <= 320);
ALTER TABLE bots ADD CONSTRAINT chk_bots_name_len CHECK (length(name) <= 100);
ALTER TABLE bots ADD CONSTRAINT chk_bots_description_len CHECK (length(description) <= 500);
ALTER TABLE bots ADD CONSTRAINT chk_bots_model_name_len CHECK (length(model_name) <= 100);
ALTER TABLE bots ADD CONSTRAINT chk_bots_model_version_len CHECK (length(model_version) <= 50);
ALTER TABLE requests ADD CONSTRAINT chk_requests_meaning_len CHECK (length(meaning) <= 2000);
ALTER TABLE kurals ADD CONSTRAINT chk_kurals_raw_text_len CHECK (length(raw_text) <= 5000);
ALTER TABLE judge_scores ADD CONSTRAINT chk_judge_scores_reasoning_len CHECK (length(reasoning) <= 2000);

-- 8. Recreate views (required after score_type enum change)
CREATE VIEW kural_scores AS
SELECT
    k.id, k.request_id, k.bot_id, k.raw_text, k.created_at,
    COALESCE(v.upvotes, 0) AS upvotes,
    COALESCE(v.downvotes, 0) AS downvotes,
    wilson_lower_bound(COALESCE(v.upvotes, 0), COALESCE(v.downvotes, 0)) AS community_score,
    js.avg_meaning,
    js.avg_prosody,
    composite_score(
        wilson_lower_bound(COALESCE(v.upvotes, 0), COALESCE(v.downvotes, 0)),
        js.avg_meaning,
        js.avg_prosody
    ) AS composite_score
FROM kurals k
LEFT JOIN (
    SELECT kural_id,
        COUNT(*) FILTER (WHERE value = 1) AS upvotes,
        COUNT(*) FILTER (WHERE value = -1) AS downvotes
    FROM kural_votes GROUP BY kural_id
) v ON v.kural_id = k.id
LEFT JOIN (
    SELECT kural_id,
        AVG(score) FILTER (WHERE score_type = 'meaning') AS avg_meaning,
        AVG(score) FILTER (WHERE score_type = 'prosody') AS avg_prosody
    FROM judge_scores GROUP BY kural_id
) js ON js.kural_id = k.id;

CREATE VIEW bot_stats AS
SELECT
    b.id, b.owner_id, b.bot_type, b.name, b.description,
    b.model_name, b.model_version, b.is_active,
    b.created_at, b.updated_at,
    COALESCE(ks.kural_count, 0) AS kural_count,
    COALESCE(ks.scored_kural_count, 0) AS scored_kural_count,
    ks.avg_composite_score
FROM bots b
LEFT JOIN (
    SELECT bot_id,
        COUNT(*) AS kural_count,
        COUNT(composite_score) AS scored_kural_count,
        AVG(composite_score) AS avg_composite_score
    FROM kural_scores
    GROUP BY bot_id
) ks ON ks.bot_id = b.id;
