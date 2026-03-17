-- ============================================================
-- Migration 004: Data model quality improvements
-- ============================================================

-- ----------------------------------------------------------------
-- 1. Fix auth_provider_id uniqueness
--    Change from globally unique to composite unique per provider.
--    Cognito sub values are unique within a pool, but the composite
--    constraint is architecturally correct for multi-provider auth.
-- ----------------------------------------------------------------
ALTER TABLE users DROP CONSTRAINT users_auth_provider_id_key;
CREATE UNIQUE INDEX idx_users_auth_provider_identity
    ON users (auth_provider, auth_provider_id);

-- ----------------------------------------------------------------
-- 2. Make ON DELETE policies explicit
--    All currently default to RESTRICT. Making this explicit
--    documents the design intent: archive/deactivate, don't delete.
-- ----------------------------------------------------------------

-- requests.author_id → users(id)
ALTER TABLE requests DROP CONSTRAINT requests_author_id_fkey;
ALTER TABLE requests ADD CONSTRAINT requests_author_id_fkey
    FOREIGN KEY (author_id) REFERENCES users(id) ON DELETE RESTRICT;

-- bots.owner_id → users(id)
ALTER TABLE bots DROP CONSTRAINT bots_owner_id_fkey;
ALTER TABLE bots ADD CONSTRAINT bots_owner_id_fkey
    FOREIGN KEY (owner_id) REFERENCES users(id) ON DELETE RESTRICT;

-- kurals.request_id → requests(id)
ALTER TABLE kurals DROP CONSTRAINT kurals_request_id_fkey;
ALTER TABLE kurals ADD CONSTRAINT kurals_request_id_fkey
    FOREIGN KEY (request_id) REFERENCES requests(id) ON DELETE RESTRICT;

-- kurals.bot_id → bots(id)
ALTER TABLE kurals DROP CONSTRAINT kurals_bot_id_fkey;
ALTER TABLE kurals ADD CONSTRAINT kurals_bot_id_fkey
    FOREIGN KEY (bot_id) REFERENCES bots(id) ON DELETE RESTRICT;

-- judge_scores.bot_id → bots(id)
ALTER TABLE judge_scores DROP CONSTRAINT judge_scores_bot_id_fkey;
ALTER TABLE judge_scores ADD CONSTRAINT judge_scores_bot_id_fkey
    FOREIGN KEY (bot_id) REFERENCES bots(id) ON DELETE RESTRICT;

-- ----------------------------------------------------------------
-- 3. Non-empty CHECK constraints
--    Defense-in-depth consistent with existing max-length CHECKs.
-- ----------------------------------------------------------------
ALTER TABLE users ADD CONSTRAINT chk_users_display_name_nonempty
    CHECK (length(trim(display_name)) > 0);
ALTER TABLE bots ADD CONSTRAINT chk_bots_name_nonempty
    CHECK (length(trim(name)) > 0);
ALTER TABLE bots ADD CONSTRAINT chk_bots_model_name_nonempty
    CHECK (length(trim(model_name)) > 0);
ALTER TABLE bots ADD CONSTRAINT chk_bots_model_version_nonempty
    CHECK (length(trim(model_version)) > 0);
ALTER TABLE bots ADD CONSTRAINT chk_bots_description_nonempty
    CHECK (description IS NULL OR length(trim(description)) > 0);
ALTER TABLE requests ADD CONSTRAINT chk_requests_meaning_nonempty
    CHECK (length(trim(meaning)) > 0);
ALTER TABLE kurals ADD CONSTRAINT chk_kurals_raw_text_nonempty
    CHECK (length(trim(raw_text)) > 0);
ALTER TABLE judge_scores ADD CONSTRAINT chk_judge_scores_reasoning_nonempty
    CHECK (reasoning IS NULL OR length(trim(reasoning)) > 0);

-- ----------------------------------------------------------------
-- 4. Constrain config keys to known values
-- ----------------------------------------------------------------
ALTER TABLE config ADD CONSTRAINT chk_config_known_keys
    CHECK (key IN ('score_weights'));
