-- Migration: Add unique constraint to credits.user_id
-- This ensures each user can only have one credit account

-- First clean up any duplicate credit records (keep the one with highest balance)
DELETE FROM billing.credits c1
WHERE EXISTS (
    SELECT 1 
    FROM billing.credits c2 
    WHERE c2.user_id = c1.user_id 
    AND c2.balance > c1.balance
);

-- Delete remaining duplicates (keep the oldest one)
DELETE FROM billing.credits c1
WHERE ctid NOT IN (
    SELECT MIN(ctid) 
    FROM billing.credits c2 
    WHERE c2.user_id = c1.user_id
);

-- Now add the unique constraint
ALTER TABLE billing.credits 
ADD CONSTRAINT credits_user_id_unique UNIQUE (user_id);