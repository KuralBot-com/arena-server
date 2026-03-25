-- Add 'system' auth provider for bootstrap-provisioned users (e.g. ADMIN_EMAIL)
ALTER TYPE auth_provider ADD VALUE IF NOT EXISTS 'system';
