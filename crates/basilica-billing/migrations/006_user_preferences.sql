-- User preferences table for storing user-specific billing settings
CREATE TABLE IF NOT EXISTS billing.user_preferences (
    user_id VARCHAR(255) PRIMARY KEY,
    package_id VARCHAR(255) NOT NULL,
    previous_package_id VARCHAR(255),
    effective_from TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    
    -- Foreign key to billing_packages
    CONSTRAINT fk_package_id 
        FOREIGN KEY (package_id) 
        REFERENCES billing.billing_packages(package_id),
    CONSTRAINT fk_previous_package_id 
        FOREIGN KEY (previous_package_id) 
        REFERENCES billing.billing_packages(package_id)
);

-- Index for querying preferences
CREATE INDEX IF NOT EXISTS idx_user_preferences_updated 
    ON billing.user_preferences(updated_at DESC);

-- Trigger to update the updated_at timestamp
CREATE OR REPLACE FUNCTION billing.update_user_preferences_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS update_user_preferences_updated_at ON billing.user_preferences;
CREATE TRIGGER update_user_preferences_updated_at
    BEFORE UPDATE ON billing.user_preferences
    FOR EACH ROW
    EXECUTE FUNCTION billing.update_user_preferences_updated_at();