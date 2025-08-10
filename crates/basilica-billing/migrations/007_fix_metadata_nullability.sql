-- Migration: Fix metadata nullability issues
-- This migration ensures metadata columns are never NULL to prevent runtime panics

-- Update any existing NULL metadata values to empty JSON objects
UPDATE billing.billing_packages 
SET metadata = '{}'::jsonb 
WHERE metadata IS NULL;

-- Add NOT NULL constraint and DEFAULT value to metadata column
ALTER TABLE billing.billing_packages 
ALTER COLUMN metadata SET NOT NULL,
ALTER COLUMN metadata SET DEFAULT '{}'::jsonb;

-- Add metadata column to user_preferences if it doesn't exist
ALTER TABLE billing.user_preferences 
ADD COLUMN IF NOT EXISTS metadata JSONB DEFAULT '{}'::jsonb;

-- Update any NULL metadata in user_preferences
UPDATE billing.user_preferences 
SET metadata = '{}'::jsonb 
WHERE metadata IS NULL;

-- Now make it NOT NULL
ALTER TABLE billing.user_preferences 
ALTER COLUMN metadata SET NOT NULL;

-- Add comment explaining the constraint
COMMENT ON COLUMN billing.billing_packages.metadata IS 'Additional package metadata as JSON, never NULL (defaults to empty object)';
COMMENT ON COLUMN billing.user_preferences.metadata IS 'User preference metadata as JSON, never NULL (defaults to empty object)';