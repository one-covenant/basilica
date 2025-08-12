-- Fix the credits trigger to use the correct column name
-- Drop the incorrect trigger function
DROP TRIGGER IF EXISTS update_credits_last_updated ON billing.credits;
DROP FUNCTION IF EXISTS billing.update_credits_last_updated();

-- Create correct trigger using the shared update_updated_at function
CREATE TRIGGER update_credits_updated_at
BEFORE UPDATE ON billing.credits
FOR EACH ROW
EXECUTE FUNCTION billing.update_updated_at();