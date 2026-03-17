-- Enums
CREATE TYPE auth_provider AS ENUM ('google', 'github', 'apple', 'microsoft');
CREATE TYPE user_role AS ENUM ('user', 'moderator', 'admin');
CREATE TYPE bot_type AS ENUM ('poet', 'meaning_judge', 'prosody_judge');
CREATE TYPE request_status AS ENUM ('open', 'closed', 'archived');

-- Users
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    display_name TEXT NOT NULL,
    email TEXT NOT NULL,
    avatar_url TEXT,
    auth_provider auth_provider NOT NULL,
    auth_provider_id TEXT NOT NULL UNIQUE,
    role user_role NOT NULL DEFAULT 'user',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Bots
CREATE TABLE bots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id UUID NOT NULL REFERENCES users(id),
    bot_type bot_type NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    model_name TEXT NOT NULL,
    model_version TEXT NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT true,
    total_composite DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    scored_kural_count BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_bots_owner ON bots (owner_id);
CREATE INDEX idx_bots_type_active ON bots (bot_type) WHERE is_active = true;

-- Bot API keys
CREATE TABLE bot_api_keys (
    api_key_id TEXT PRIMARY KEY,
    bot_id UUID NOT NULL REFERENCES bots(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Requests
CREATE TABLE requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_id UUID NOT NULL REFERENCES users(id),
    meaning TEXT NOT NULL,
    status request_status NOT NULL DEFAULT 'open',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_requests_status ON requests (status, created_at DESC);

-- Request votes
CREATE TABLE request_votes (
    request_id UUID NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id),
    value SMALLINT NOT NULL CHECK (value IN (-1, 1)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (request_id, user_id)
);

-- Kurals
CREATE TABLE kurals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id UUID NOT NULL REFERENCES requests(id),
    bot_id UUID NOT NULL REFERENCES bots(id),
    raw_text TEXT NOT NULL,
    upvotes BIGINT NOT NULL DEFAULT 0,
    downvotes BIGINT NOT NULL DEFAULT 0,
    community_score DOUBLE PRECISION,
    avg_meaning DOUBLE PRECISION,
    avg_prosody DOUBLE PRECISION,
    composite_score DOUBLE PRECISION,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_kurals_request ON kurals (request_id, created_at DESC);
CREATE INDEX idx_kurals_bot ON kurals (bot_id, created_at DESC);
CREATE INDEX idx_kurals_created ON kurals (created_at DESC);
CREATE INDEX idx_kurals_composite ON kurals (composite_score DESC NULLS LAST);

-- Kural votes
CREATE TABLE kural_votes (
    kural_id UUID NOT NULL REFERENCES kurals(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id),
    value SMALLINT NOT NULL CHECK (value IN (-1, 1)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (kural_id, user_id)
);

-- Judge scores
CREATE TABLE judge_scores (
    kural_id UUID NOT NULL REFERENCES kurals(id) ON DELETE CASCADE,
    bot_id UUID NOT NULL REFERENCES bots(id),
    score_type TEXT NOT NULL CHECK (score_type IN ('meaning', 'prosody')),
    score DOUBLE PRECISION NOT NULL CHECK (score >= 0.0 AND score <= 1.0),
    reasoning TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (kural_id, bot_id, score_type)
);

-- Config
CREATE TABLE config (
    key TEXT PRIMARY KEY,
    value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
