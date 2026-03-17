-- Wilson lower bound function (95% CI)
CREATE FUNCTION wilson_lower_bound(up BIGINT, down BIGINT)
RETURNS DOUBLE PRECISION AS $$
DECLARE
    n DOUBLE PRECISION;
    p_hat DOUBLE PRECISION;
    z DOUBLE PRECISION := 1.96;
BEGIN
    n := up + down;
    IF n = 0 THEN RETURN NULL; END IF;
    p_hat := up::DOUBLE PRECISION / n;
    RETURN (p_hat + z*z/(2*n) - z * sqrt((p_hat*(1-p_hat) + z*z/(4*n))/n)) / (1 + z*z/n);
END;
$$ LANGUAGE plpgsql IMMUTABLE;

-- Composite score function (weighted average, normalized to 0-100)
CREATE FUNCTION composite_score(
    community DOUBLE PRECISION,
    meaning DOUBLE PRECISION,
    prosody DOUBLE PRECISION,
    w_community REAL DEFAULT 0.34,
    w_meaning REAL DEFAULT 0.33,
    w_prosody REAL DEFAULT 0.33
) RETURNS DOUBLE PRECISION AS $$
DECLARE
    weighted_sum DOUBLE PRECISION := 0;
    total_weight DOUBLE PRECISION := 0;
BEGIN
    IF community IS NOT NULL THEN
        weighted_sum := weighted_sum + community * w_community;
        total_weight := total_weight + w_community;
    END IF;
    IF meaning IS NOT NULL THEN
        weighted_sum := weighted_sum + meaning * w_meaning;
        total_weight := total_weight + w_meaning;
    END IF;
    IF prosody IS NOT NULL THEN
        weighted_sum := weighted_sum + prosody * w_prosody;
        total_weight := total_weight + w_prosody;
    END IF;
    IF total_weight = 0 THEN RETURN NULL; END IF;
    RETURN weighted_sum / total_weight * 100.0;
END;
$$ LANGUAGE plpgsql IMMUTABLE;

-- View: kural_scores — computes all derived kural fields from source-of-truth tables
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

-- View: bot_stats — computes bot aggregate fields from kural_scores
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

-- Drop derived columns from kurals
ALTER TABLE kurals
    DROP COLUMN upvotes,
    DROP COLUMN downvotes,
    DROP COLUMN community_score,
    DROP COLUMN avg_meaning,
    DROP COLUMN avg_prosody,
    DROP COLUMN composite_score;

-- Drop derived columns from bots
ALTER TABLE bots
    DROP COLUMN total_composite,
    DROP COLUMN scored_kural_count;

-- Drop now-redundant index (was on the dropped column)
DROP INDEX IF EXISTS idx_kurals_composite;

-- Add indexes for view subquery performance
CREATE INDEX idx_kural_votes_kural_value ON kural_votes (kural_id, value);
CREATE INDEX idx_judge_scores_kural_type ON judge_scores (kural_id, score_type);
