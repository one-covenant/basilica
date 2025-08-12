-- Initial schema for Basilica Billing Service
-- Create extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pgcrypto";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";
-- Create schema
CREATE SCHEMA IF NOT EXISTS billing;
-- Users table (minimal, most data from Auth0)
CREATE TABLE IF NOT EXISTS billing.users (
  user_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  external_id VARCHAR(255) UNIQUE NOT NULL,
  -- Auth0 user ID
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);
-- Credits table
CREATE TABLE IF NOT EXISTS billing.credits (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES billing.users(user_id),
  balance DECIMAL(20, 8) NOT NULL DEFAULT 0,
  reserved_balance DECIMAL(20, 8) NOT NULL DEFAULT 0,
  lifetime_credits DECIMAL(20, 8) NOT NULL DEFAULT 0,
  lifetime_spent DECIMAL(20, 8) NOT NULL DEFAULT 0,
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  version INTEGER NOT NULL DEFAULT 1,
  -- For optimistic locking
  CONSTRAINT positive_balance CHECK (balance >= 0),
  CONSTRAINT positive_reserved CHECK (reserved_balance >= 0),
  CONSTRAINT credits_user_id_unique UNIQUE (user_id)
);
-- Credit transactions for audit trail
CREATE TABLE IF NOT EXISTS billing.credit_transactions (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES billing.users(user_id),
  transaction_type VARCHAR(50) NOT NULL,
  -- 'credit', 'debit', 'reserve', 'release'
  amount DECIMAL(20, 8) NOT NULL,
  balance_before DECIMAL(20, 8) NOT NULL,
  balance_after DECIMAL(20, 8) NOT NULL,
  reference_id VARCHAR(255),
  -- External reference (payment ID, rental ID, etc.)
  reference_type VARCHAR(50),
  -- 'payment', 'rental', 'refund', etc.
  description TEXT,
  metadata JSONB,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);
-- Credit reservations
CREATE TABLE IF NOT EXISTS billing.credit_reservations (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES billing.users(user_id),
  rental_id UUID NOT NULL,
  amount DECIMAL(20, 8) NOT NULL,
  status VARCHAR(20) NOT NULL DEFAULT 'active',
  -- 'active', 'released', 'charged'
  reserved_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  expires_at TIMESTAMP WITH TIME ZONE NOT NULL,
  released_at TIMESTAMP WITH TIME ZONE,
  final_amount DECIMAL(20, 8),
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);
-- Billing packages
CREATE TABLE IF NOT EXISTS billing.billing_packages (
  package_id VARCHAR(100) PRIMARY KEY,
  name VARCHAR(255) NOT NULL,
  description TEXT,
  base_rate_per_hour DECIMAL(10, 8) NOT NULL DEFAULT 0,
  cpu_rate_per_hour DECIMAL(10, 8) NOT NULL,
  memory_rate_per_gb_hour DECIMAL(10, 8) NOT NULL,
  network_rate_per_gb DECIMAL(10, 8) NOT NULL,
  disk_iops_rate DECIMAL(10, 8) NOT NULL,
  included_cpu_hours INTEGER DEFAULT 0,
  included_memory_gb_hours INTEGER DEFAULT 0,
  included_network_gb INTEGER DEFAULT 0,
  included_disk_iops INTEGER DEFAULT 0,
  priority INTEGER NOT NULL DEFAULT 0,
  is_active BOOLEAN DEFAULT true,
  metadata JSONB,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);
-- GPU rates per package
CREATE TABLE IF NOT EXISTS billing.package_gpu_rates (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  package_id VARCHAR(100) REFERENCES billing.billing_packages(package_id) ON DELETE CASCADE,
  gpu_model VARCHAR(100) NOT NULL,
  rate_per_hour DECIMAL(10, 8) NOT NULL,
  included_hours INTEGER DEFAULT 0,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  UNIQUE(package_id, gpu_model)
);
-- User package assignments
CREATE TABLE IF NOT EXISTS billing.user_packages (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES billing.users(user_id),
  package_id VARCHAR(100) NOT NULL REFERENCES billing.billing_packages(package_id),
  effective_from TIMESTAMP WITH TIME ZONE NOT NULL,
  effective_until TIMESTAMP WITH TIME ZONE,
  is_active BOOLEAN DEFAULT true,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);
-- Custom billing rules
CREATE TABLE IF NOT EXISTS billing.billing_rules (
  rule_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  package_id VARCHAR(100) REFERENCES billing.billing_packages(package_id),
  name VARCHAR(255) NOT NULL,
  description TEXT,
  rule_type VARCHAR(50) NOT NULL,
  -- 'discount', 'surcharge', 'override'
  condition_type VARCHAR(50) NOT NULL,
  -- 'time_based', 'usage_based', 'user_based'
  condition_data JSONB NOT NULL,
  action_type VARCHAR(50) NOT NULL,
  -- 'percentage', 'fixed', 'rate_override'
  action_data JSONB NOT NULL,
  priority INTEGER NOT NULL DEFAULT 0,
  is_active BOOLEAN DEFAULT true,
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);
-- Indexes
CREATE INDEX IF NOT EXISTS idx_credits_user_id ON billing.credits(user_id);
CREATE INDEX IF NOT EXISTS idx_credit_transactions_user_id ON billing.credit_transactions(user_id);
CREATE INDEX IF NOT EXISTS idx_credit_transactions_created_at ON billing.credit_transactions(created_at);
CREATE INDEX IF NOT EXISTS idx_credit_transactions_reference ON billing.credit_transactions(reference_id, reference_type);
CREATE INDEX IF NOT EXISTS idx_credit_reservations_user_id ON billing.credit_reservations(user_id);
CREATE INDEX IF NOT EXISTS idx_credit_reservations_rental_id ON billing.credit_reservations(rental_id);
CREATE INDEX IF NOT EXISTS idx_credit_reservations_status ON billing.credit_reservations(status);
CREATE INDEX IF NOT EXISTS idx_user_packages_user_id ON billing.user_packages(user_id);
CREATE INDEX IF NOT EXISTS idx_user_packages_active ON billing.user_packages(user_id, is_active)
WHERE is_active = true;
CREATE INDEX IF NOT EXISTS idx_billing_rules_package_id ON billing.billing_rules(package_id);
CREATE INDEX IF NOT EXISTS idx_billing_rules_active ON billing.billing_rules(is_active)
WHERE is_active = true;
-- Triggers for updated_at
CREATE OR REPLACE FUNCTION billing.update_updated_at() RETURNS TRIGGER AS $$ BEGIN NEW.updated_at = NOW();
RETURN NEW;
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS update_users_updated_at ON billing.users;
CREATE TRIGGER update_users_updated_at BEFORE
UPDATE ON billing.users FOR EACH ROW EXECUTE FUNCTION billing.update_updated_at();
DROP TRIGGER IF EXISTS update_credits_updated_at ON billing.credits;
CREATE TRIGGER update_credits_updated_at BEFORE
UPDATE ON billing.credits FOR EACH ROW EXECUTE FUNCTION billing.update_updated_at();
DROP TRIGGER IF EXISTS update_credit_reservations_updated_at ON billing.credit_reservations;
CREATE TRIGGER update_credit_reservations_updated_at BEFORE
UPDATE ON billing.credit_reservations FOR EACH ROW EXECUTE FUNCTION billing.update_updated_at();
DROP TRIGGER IF EXISTS update_billing_packages_updated_at ON billing.billing_packages;
CREATE TRIGGER update_billing_packages_updated_at BEFORE
UPDATE ON billing.billing_packages FOR EACH ROW EXECUTE FUNCTION billing.update_updated_at();
DROP TRIGGER IF EXISTS update_user_packages_updated_at ON billing.user_packages;
CREATE TRIGGER update_user_packages_updated_at BEFORE
UPDATE ON billing.user_packages FOR EACH ROW EXECUTE FUNCTION billing.update_updated_at();
DROP TRIGGER IF EXISTS update_billing_rules_updated_at ON billing.billing_rules;
CREATE TRIGGER update_billing_rules_updated_at BEFORE
UPDATE ON billing.billing_rules FOR EACH ROW EXECUTE FUNCTION billing.update_updated_at();
