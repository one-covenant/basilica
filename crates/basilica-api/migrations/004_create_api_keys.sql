-- Create API keys table for Personal Access Tokens
CREATE TABLE api_keys (
    id BIGSERIAL PRIMARY KEY,
    user_id TEXT NOT NULL,
    kid VARCHAR(16) NOT NULL UNIQUE,
    name TEXT NOT NULL,
    hash TEXT NOT NULL,
    scopes TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ NULL,
    last_used_at TIMESTAMPTZ NULL
);

-- Index for user lookups
CREATE INDEX idx_api_keys_user_id ON api_keys(user_id);

-- Index for active keys
CREATE INDEX idx_api_keys_active ON api_keys((revoked_at IS NULL)) WHERE revoked_at IS NULL;

-- Unique index to enforce one active key per user
CREATE UNIQUE INDEX uq_api_keys_user_active ON api_keys(user_id) WHERE revoked_at IS NULL;