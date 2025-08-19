-- Create user_rentals table for tracking rental ownership
CREATE TABLE IF NOT EXISTS user_rentals (
    rental_id VARCHAR(255) PRIMARY KEY,
    user_id VARCHAR(255) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Index for efficient user-based queries
CREATE INDEX IF NOT EXISTS idx_user_rentals_user_id ON user_rentals(user_id);

-- Index for time-based queries (for cleanup, analytics, etc.)
CREATE INDEX IF NOT EXISTS idx_user_rentals_created_at ON user_rentals(created_at);