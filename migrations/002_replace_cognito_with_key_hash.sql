-- Replace Cognito/API Gateway credential columns with local API key hash.
-- Existing credentials are invalidated since they relied on AWS Cognito.

-- Revoke all existing credentials (they used Cognito and are no longer valid)
UPDATE agent_credentials SET is_active = false, revoked_at = now() WHERE is_active = true;

-- Drop the old Cognito-specific index
DROP INDEX IF EXISTS idx_agent_credentials_cognito;

-- Remove AWS-specific columns
ALTER TABLE agent_credentials DROP COLUMN cognito_client_id;
ALTER TABLE agent_credentials DROP COLUMN api_gw_key_id;

-- Add the key hash column (SHA-256 hex digest of the plaintext API key)
ALTER TABLE agent_credentials ADD COLUMN key_hash TEXT;

-- Unique index on key_hash for fast lookups during auth
CREATE UNIQUE INDEX idx_agent_credentials_key_hash ON agent_credentials (key_hash) WHERE key_hash IS NOT NULL;
