-- Migration: Update billing packages table with simplified structure
-- Add columns that don't exist yet for simplified billing
ALTER TABLE billing.billing_packages 
ADD COLUMN IF NOT EXISTS hourly_rate DECIMAL(10, 4) NOT NULL DEFAULT 0 CHECK (hourly_rate >= 0);

ALTER TABLE billing.billing_packages 
ADD COLUMN IF NOT EXISTS gpu_model VARCHAR(100) NOT NULL DEFAULT '';

ALTER TABLE billing.billing_packages 
ADD COLUMN IF NOT EXISTS billing_period VARCHAR(50) NOT NULL DEFAULT 'Hourly';

ALTER TABLE billing.billing_packages 
ADD COLUMN IF NOT EXISTS active BOOLEAN NOT NULL DEFAULT true;

-- Update package_id column size if needed (can't alter PRIMARY KEY easily, so skip if exists)
-- ALTER TABLE billing.billing_packages ALTER COLUMN package_id TYPE VARCHAR(255);
-- Create index for active packages lookup
CREATE INDEX IF NOT EXISTS idx_billing_packages_active ON billing.billing_packages(active)
WHERE active = true;
-- Create index for priority ordering
CREATE INDEX IF NOT EXISTS idx_billing_packages_priority ON billing.billing_packages(priority, package_id);
-- Create index for GPU model lookup
CREATE INDEX IF NOT EXISTS idx_billing_packages_gpu_model ON billing.billing_packages(gpu_model);
-- Trigger already exists from migration 001, skip creating it again
-- Insert default packages with all required columns
INSERT INTO billing.billing_packages (
    package_id,
    name,
    description,
    base_rate_per_hour,
    cpu_rate_per_hour,
    memory_rate_per_gb_hour,
    network_rate_per_gb,
    disk_iops_rate,
    hourly_rate,
    gpu_model,
    billing_period,
    priority,
    active
  )
VALUES (
    'h100',
    'H100 GPU',
    'NVIDIA H100 GPU instances - $3.50/hour',
    3.50,
    0.01,
    0.001,
    0.001,
    0.0001,
    3.50,
    'H100',
    'Hourly',
    100,
    true
  ),
  (
    'h200',
    'H200 GPU',
    'NVIDIA H200 GPU instances - $5.00/hour',
    5.00,
    0.01,
    0.001,
    0.001,
    0.0001,
    5.00,
    'H200',
    'Hourly',
    200,
    true
  ),
  (
    'custom',
    'Custom',
    'Custom pricing for bespoke deals',
    0.00,
    0.01,
    0.001,
    0.001,
    0.0001,
    0.00,
    'Custom',
    'Hourly',
    300,
    true
  ) ON CONFLICT (package_id) DO UPDATE SET
    name = EXCLUDED.name,
    description = EXCLUDED.description,
    hourly_rate = EXCLUDED.hourly_rate,
    cpu_rate_per_hour = EXCLUDED.cpu_rate_per_hour,
    memory_rate_per_gb_hour = EXCLUDED.memory_rate_per_gb_hour,
    network_rate_per_gb = EXCLUDED.network_rate_per_gb,
    disk_iops_rate = EXCLUDED.disk_iops_rate,
    base_rate_per_hour = EXCLUDED.base_rate_per_hour,
    gpu_model = EXCLUDED.gpu_model,
    billing_period = EXCLUDED.billing_period,
    priority = EXCLUDED.priority,
    active = EXCLUDED.active,
    updated_at = NOW();
-- Add comment to table
COMMENT ON TABLE billing.billing_packages IS 'Stores available billing packages for GPU rentals';
COMMENT ON COLUMN billing.billing_packages.package_id IS 'Unique identifier for the package';
COMMENT ON COLUMN billing.billing_packages.hourly_rate IS 'Base hourly rate in credits';
COMMENT ON COLUMN billing.billing_packages.gpu_model IS 'Type of GPU (H100, H200, Custom)';
COMMENT ON COLUMN billing.billing_packages.billing_period IS 'Billing period type (Hourly, Daily, Weekly, Monthly)';
COMMENT ON COLUMN billing.billing_packages.priority IS 'Display priority (lower numbers shown first)';
COMMENT ON COLUMN billing.billing_packages.metadata IS 'Additional package configuration as JSON';
