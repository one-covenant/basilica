-- Fix rental_id to be nullable in credit_reservations table
-- A credit reservation may not always have an associated rental

ALTER TABLE billing.credit_reservations 
ALTER COLUMN rental_id DROP NOT NULL;