-- Add SSH credentials column to user_rentals table
ALTER TABLE user_rentals 
ADD COLUMN ssh_credentials TEXT;

-- The column is nullable to support existing rentals
-- and rentals created with no_ssh flag