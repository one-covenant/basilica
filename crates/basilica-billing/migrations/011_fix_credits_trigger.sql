-- Fix the update trigger for credits table to use correct column name
-- The credits table has 'last_updated' not 'updated_at'
-- Drop the incorrect trigger
DROP TRIGGER IF EXISTS update_credits_updated_at ON billing.credits;
-- Create a specific function for credits table
CREATE OR REPLACE FUNCTION billing.update_credits_last_updated() RETURNS TRIGGER AS $$ BEGIN NEW.last_updated = NOW();
RETURN NEW;
END;
$$ LANGUAGE plpgsql;
-- Create the trigger with the correct function
CREATE TRIGGER update_credits_last_updated BEFORE
UPDATE ON billing.credits FOR EACH ROW EXECUTE FUNCTION billing.update_credits_last_updated();
