-- Create API keys table for Personal Access Tokens
CREATE TABLE api_keys (
    user_id TEXT NOT NULL,
    kid VARCHAR(16) NOT NULL,
    name TEXT NOT NULL,
    hash TEXT NOT NULL,
    scopes TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ NULL,
    PRIMARY KEY (user_id, kid),
    UNIQUE(user_id, name) -- Ensures unique names per user
);

-- Index for fast authentication lookup by kid
CREATE INDEX idx_api_keys_kid ON api_keys(kid);