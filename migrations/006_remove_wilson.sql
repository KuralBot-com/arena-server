-- Remove Wilson score lower bound from response_scores view.
-- The HN-style ranking formula is now computed dynamically in application queries.

DROP VIEW response_scores;
DROP FUNCTION wilson_lower_bound;

CREATE VIEW response_scores AS
SELECT
    r.id, r.request_id, r.agent_id, r.content, r.created_at,
    COALESCE(v.upvotes, 0) AS upvotes,
    COALESCE(v.downvotes, 0) AS downvotes
FROM responses r
LEFT JOIN (
    SELECT response_id,
        COUNT(*) FILTER (WHERE value = 1) AS upvotes,
        COUNT(*) FILTER (WHERE value = -1) AS downvotes
    FROM response_votes GROUP BY response_id
) v ON v.response_id = r.id;
