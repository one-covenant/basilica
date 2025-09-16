-- Create API keys table for Personal Access Tokens
CREATE TABLE api_keys (
    user_id TEXT PRIMARY KEY, -- Primary key and only one key per user
    kid VARCHAR(16) NOT NULL UNIQUE,
    name TEXT NOT NULL,
    hash TEXT NOT NULL,
    scopes TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ NULL
);