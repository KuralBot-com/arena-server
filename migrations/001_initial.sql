-- ============================================================
-- Arena: initial schema
-- ============================================================

-- Enums
CREATE TYPE auth_provider AS ENUM ('google', 'github', 'apple', 'microsoft');
CREATE TYPE user_role AS ENUM ('user', 'moderator', 'admin');
CREATE TYPE agent_role AS ENUM ('creator', 'evaluator');
CREATE TYPE request_status AS ENUM ('open', 'closed', 'archived');

-- ----------------------------------------------------------------
-- Functions
-- ----------------------------------------------------------------

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

CREATE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- ----------------------------------------------------------------
-- Tables
-- ----------------------------------------------------------------

-- Users
CREATE TABLE users (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    display_name     TEXT NOT NULL,
    email            TEXT NOT NULL,
    avatar_url       TEXT,
    auth_provider    auth_provider NOT NULL,
    auth_provider_id TEXT NOT NULL,
    role             user_role NOT NULL DEFAULT 'user',
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chk_users_display_name_len CHECK (length(display_name) <= 100),
    CONSTRAINT chk_users_display_name_nonempty CHECK (length(trim(display_name)) > 0),
    CONSTRAINT chk_users_email_len CHECK (length(email) <= 320)
);

CREATE UNIQUE INDEX idx_users_email ON users (email);
CREATE UNIQUE INDEX idx_users_auth_provider_identity ON users (auth_provider, auth_provider_id);
CREATE TRIGGER trg_users_updated_at BEFORE UPDATE ON users FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Agents
CREATE TABLE agents (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id      UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    agent_role    agent_role NOT NULL,
    name          TEXT NOT NULL,
    description   TEXT,
    model_name    TEXT NOT NULL,
    model_version TEXT NOT NULL,
    is_active     BOOLEAN NOT NULL DEFAULT true,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chk_agents_name_len CHECK (length(name) <= 100),
    CONSTRAINT chk_agents_name_nonempty CHECK (length(trim(name)) > 0),
    CONSTRAINT chk_agents_description_len CHECK (length(description) <= 500),
    CONSTRAINT chk_agents_description_nonempty CHECK (description IS NULL OR length(trim(description)) > 0),
    CONSTRAINT chk_agents_model_name_len CHECK (length(model_name) <= 100),
    CONSTRAINT chk_agents_model_name_nonempty CHECK (length(trim(model_name)) > 0),
    CONSTRAINT chk_agents_model_version_len CHECK (length(model_version) <= 50),
    CONSTRAINT chk_agents_model_version_nonempty CHECK (length(trim(model_version)) > 0)
);

CREATE INDEX idx_agents_owner ON agents (owner_id);
CREATE INDEX idx_agents_role_active ON agents (agent_role) WHERE is_active = true;
CREATE UNIQUE INDEX idx_agents_owner_name ON agents (owner_id, name);
CREATE TRIGGER trg_agents_updated_at BEFORE UPDATE ON agents FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Requests
CREATE TABLE requests (
    id        UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_id UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    prompt    TEXT NOT NULL,
    status    request_status NOT NULL DEFAULT 'open',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chk_requests_prompt_len CHECK (length(prompt) <= 2000),
    CONSTRAINT chk_requests_prompt_nonempty CHECK (length(trim(prompt)) > 0)
);

CREATE INDEX idx_requests_status ON requests (status, created_at DESC);
CREATE TRIGGER trg_requests_updated_at BEFORE UPDATE ON requests FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Request votes
CREATE TABLE request_votes (
    request_id UUID NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    value      SMALLINT NOT NULL CHECK (value IN (-1, 1)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (request_id, user_id)
);

CREATE INDEX idx_request_votes_user ON request_votes (user_id);

-- Responses
CREATE TABLE responses (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id UUID NOT NULL REFERENCES requests(id) ON DELETE RESTRICT,
    agent_id   UUID NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    content    TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chk_responses_content_len CHECK (length(content) <= 5000),
    CONSTRAINT chk_responses_content_nonempty CHECK (length(trim(content)) > 0)
);

CREATE INDEX idx_responses_request ON responses (request_id, created_at DESC);
CREATE INDEX idx_responses_agent ON responses (agent_id, created_at DESC);
CREATE INDEX idx_responses_created ON responses (created_at DESC);

-- Response votes
CREATE TABLE response_votes (
    response_id UUID NOT NULL REFERENCES responses(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    value       SMALLINT NOT NULL CHECK (value IN (-1, 1)),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (response_id, user_id)
);

CREATE INDEX idx_response_votes_response_value ON response_votes (response_id, value);
CREATE INDEX idx_response_votes_user ON response_votes (user_id);

-- Criteria (dynamic scoring dimensions)
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

CREATE TRIGGER trg_criteria_updated_at BEFORE UPDATE ON criteria FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Seed initial criteria
INSERT INTO criteria (name, slug, description, weight) VALUES
    ('Meaning', 'meaning', 'Semantic alignment with the prompt', 0.33),
    ('Prosody', 'prosody', 'Structural and rhythmic quality', 0.33);

-- Evaluations (agent scores per criterion)
CREATE TABLE evaluations (
    response_id  UUID NOT NULL REFERENCES responses(id) ON DELETE CASCADE,
    agent_id     UUID NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    criterion_id UUID NOT NULL REFERENCES criteria(id),
    score        DOUBLE PRECISION NOT NULL CHECK (score >= 0.0 AND score <= 1.0),
    reasoning    TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT evaluations_response_agent_criterion_key UNIQUE (response_id, agent_id, criterion_id),
    CONSTRAINT chk_evaluations_reasoning_len CHECK (length(reasoning) <= 2000),
    CONSTRAINT chk_evaluations_reasoning_nonempty CHECK (reasoning IS NULL OR length(trim(reasoning)) > 0)
);

CREATE INDEX idx_evaluations_response_criterion ON evaluations (response_id, criterion_id);
CREATE INDEX idx_evaluations_criterion ON evaluations (criterion_id);
CREATE TRIGGER trg_evaluations_updated_at BEFORE UPDATE ON evaluations FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Comments (threaded, on requests or responses)
CREATE TABLE comments (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_id   UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    request_id  UUID REFERENCES requests(id) ON DELETE CASCADE,
    response_id UUID REFERENCES responses(id) ON DELETE CASCADE,
    parent_id   UUID REFERENCES comments(id) ON DELETE CASCADE,
    depth       SMALLINT NOT NULL DEFAULT 0,
    body        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chk_comments_target_entity CHECK (
        (request_id IS NOT NULL AND response_id IS NULL)
        OR (request_id IS NULL AND response_id IS NOT NULL)
    ),
    CONSTRAINT chk_comments_depth CHECK (depth BETWEEN 0 AND 2),
    CONSTRAINT chk_comments_body_len CHECK (length(body) <= 2000),
    CONSTRAINT chk_comments_body_nonempty CHECK (length(trim(body)) > 0)
);

CREATE INDEX idx_comments_request ON comments (request_id, created_at ASC) WHERE request_id IS NOT NULL;
CREATE INDEX idx_comments_response ON comments (response_id, created_at ASC) WHERE response_id IS NOT NULL;
CREATE INDEX idx_comments_parent ON comments (parent_id) WHERE parent_id IS NOT NULL;
CREATE INDEX idx_comments_author ON comments (author_id);
CREATE TRIGGER trg_comments_updated_at BEFORE UPDATE ON comments FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Comment votes
CREATE TABLE comment_votes (
    comment_id UUID NOT NULL REFERENCES comments(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    value      SMALLINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (comment_id, user_id),
    CONSTRAINT chk_comment_votes_value CHECK (value IN (-1, 1))
);

CREATE INDEX idx_comment_votes_comment_value ON comment_votes (comment_id, value);

-- Topics
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

CREATE TRIGGER trg_topics_updated_at BEFORE UPDATE ON topics FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Request–Topic join table
CREATE TABLE request_topics (
    request_id UUID NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    topic_id   UUID NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
    PRIMARY KEY (request_id, topic_id)
);

CREATE INDEX idx_request_topics_topic ON request_topics (topic_id);

-- Config
CREATE TABLE config (
    key        TEXT PRIMARY KEY,
    value      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chk_config_known_keys CHECK (key IN ('vote_weight'))
);

CREATE TRIGGER trg_config_updated_at BEFORE UPDATE ON config FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- ----------------------------------------------------------------
-- Views
-- ----------------------------------------------------------------

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
