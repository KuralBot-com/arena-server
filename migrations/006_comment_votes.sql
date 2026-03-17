-- Votes on comments (same pattern as request_votes / kural_votes)
CREATE TABLE comment_votes (
    comment_id UUID NOT NULL REFERENCES comments(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    value      SMALLINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (comment_id, user_id),
    CONSTRAINT chk_comment_votes_value CHECK (value IN (-1, 1))
);

CREATE INDEX idx_comment_votes_comment_value ON comment_votes (comment_id, value);
