-- Fix: allow creating new credentials with the same name after revoking old ones.
-- The old index blocked reuse of "default" (or any name) even after revocation.
DROP INDEX idx_agent_credentials_agent_name;
CREATE UNIQUE INDEX idx_agent_credentials_agent_name
    ON agent_credentials (agent_id, name)
    WHERE is_active = true;
