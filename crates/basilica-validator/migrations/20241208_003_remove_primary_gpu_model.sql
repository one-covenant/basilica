-- Remove primary_gpu_model column from miner_gpu_profiles table
-- First drop the index that depends on this column
DROP INDEX IF EXISTS idx_gpu_profiles_model;

-- Remove the primary_gpu_model column
ALTER TABLE miner_gpu_profiles DROP COLUMN primary_gpu_model; 