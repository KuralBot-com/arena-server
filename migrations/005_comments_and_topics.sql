-- Comments: threaded comments on requests and kurals
CREATE TABLE comments (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_id   UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    request_id  UUID REFERENCES requests(id) ON DELETE CASCADE,
    kural_id    UUID REFERENCES kurals(id) ON DELETE CASCADE,
    parent_id   UUID REFERENCES comments(id) ON DELETE CASCADE,
    depth       SMALLINT NOT NULL DEFAULT 0,
    body        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chk_comments_target CHECK (
        (request_id IS NOT NULL AND kural_id IS NULL)
        OR (request_id IS NULL AND kural_id IS NOT NULL)
    ),
    CONSTRAINT chk_comments_depth CHECK (depth BETWEEN 0 AND 2),
    CONSTRAINT chk_comments_body_len CHECK (length(body) <= 2000),
    CONSTRAINT chk_comments_body_nonempty CHECK (length(trim(body)) > 0)
);

CREATE INDEX idx_comments_request ON comments (request_id, created_at ASC) WHERE request_id IS NOT NULL;
CREATE INDEX idx_comments_kural ON comments (kural_id, created_at ASC) WHERE kural_id IS NOT NULL;
CREATE INDEX idx_comments_parent ON comments (parent_id) WHERE parent_id IS NOT NULL;
CREATE INDEX idx_comments_author ON comments (author_id);

CREATE TRIGGER trg_comments_updated_at
    BEFORE UPDATE ON comments
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Topics: curated tags for requests
CREATE TABLE topics (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    slug        TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chk_topics_name_len CHECK (length(name) <= 50),
    CONSTRAINT chk_topics_name_nonempty CHECK (length(trim(name)) > 0),
    CONSTRAINT chk_topics_slug_len CHECK (length(slug) <= 60),
    CONSTRAINT chk_topics_slug_nonempty CHECK (length(trim(slug)) > 0),
    CONSTRAINT chk_topics_description_len CHECK (description IS NULL OR length(description) <= 500),
    CONSTRAINT chk_topics_description_nonempty CHECK (description IS NULL OR length(trim(description)) > 0)
);

CREATE TRIGGER trg_topics_updated_at
    BEFORE UPDATE ON topics
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Many-to-many: requests <-> topics
CREATE TABLE request_topics (
    request_id UUID NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    topic_id   UUID NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
    PRIMARY KEY (request_id, topic_id)
);

CREATE INDEX idx_request_topics_topic ON request_topics (topic_id);
