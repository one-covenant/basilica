-- Add required user_id and validator_id columns to usage_events table
-- These fields are CRITICAL for proper billing and auditing

-- Add user_id column (required for billing)
ALTER TABLE billing.usage_events 
ADD COLUMN IF NOT EXISTS user_id VARCHAR(255) NOT NULL DEFAULT '';

-- Add validator_id column (required for tracking which validator managed the rental)
ALTER TABLE billing.usage_events 
ADD COLUMN IF NOT EXISTS validator_id VARCHAR(255) NOT NULL DEFAULT '';

-- Create indexes for the new columns for query performance
CREATE INDEX IF NOT EXISTS idx_usage_events_user_id ON billing.usage_events(user_id);
CREATE INDEX IF NOT EXISTS idx_usage_events_validator_id ON billing.usage_events(validator_id);

-- Also add validator_id to active_rentals_facts if missing
ALTER TABLE billing.active_rentals_facts
ADD COLUMN IF NOT EXISTS validator_id VARCHAR(255) NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_active_rentals_validator_id ON billing.active_rentals_facts(validator_id);

-- Add comment explaining criticality
COMMENT ON COLUMN billing.usage_events.user_id IS 'REQUIRED: User who owns this rental - must never be empty';
COMMENT ON COLUMN billing.usage_events.validator_id IS 'REQUIRED: Validator managing this rental - must never be empty';
COMMENT ON COLUMN billing.usage_events.executor_id IS 'REQUIRED: Executor running the workload - must never be empty';