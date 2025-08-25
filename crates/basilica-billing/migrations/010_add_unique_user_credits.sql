-- Clean up any duplicate credit records
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
