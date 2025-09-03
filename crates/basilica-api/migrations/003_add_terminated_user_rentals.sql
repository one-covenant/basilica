-- Create terminated_user_rentals table for tracking stopped/terminated rental history
CREATE TABLE IF NOT EXISTS terminated_user_rentals (
    rental_id VARCHAR(255) PRIMARY KEY,
    user_id VARCHAR(255) NOT NULL,
    ssh_credentials TEXT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL,
    stopped_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    stop_reason TEXT
);

-- Index for efficient user-based queries on historical rentals
CREATE INDEX IF NOT EXISTS idx_terminated_user_rentals_user_id ON terminated_user_rentals(user_id);

-- Index for time-based queries (for analytics, auditing, etc.)
CREATE INDEX IF NOT EXISTS idx_terminated_user_rentals_stopped_at ON terminated_user_rentals(stopped_at);

-- Composite index for user + time queries (e.g., user's rental history in date range)
CREATE INDEX IF NOT EXISTS idx_terminated_user_rentals_user_stopped ON terminated_user_rentals(user_id, stopped_at DESC);