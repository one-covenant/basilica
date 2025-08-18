
# Generate random password for Aurora PostgreSQL (excludes /, @, ", and space)
resource "random_password" "db_password" {
  length           = 32
  special          = true
  override_special = "!#$%^&*()-_=+{}[]|:;<>?,.~"
}

# Generate random AEAD key for payments encryption (32 bytes = 256 bits)
resource "random_bytes" "payments_aead_key" {
  length = 32
}

# Secrets Manager secret for payments AEAD encryption key
resource "aws_secretsmanager_secret" "payments_aead_key" {
  name                    = "${local.name_prefix}-payments-aead-key"
  description             = "AEAD encryption key for payments service"
  recovery_window_in_days = 7

  tags = merge(local.common_tags, {
    Name = "${local.name_prefix}-payments-aead-key"
  })
}

resource "aws_secretsmanager_secret_version" "payments_aead_key" {
  secret_id     = aws_secretsmanager_secret.payments_aead_key.id
  secret_string = random_bytes.payments_aead_key.hex
}

# Billing service database secret (separate from RDS secret for compatibility)
resource "aws_secretsmanager_secret" "billing_database" {
  name                    = "basilica-v3/billing/database"
  description             = "Database credentials for billing service"
  recovery_window_in_days = 7

  tags = merge(local.common_tags, {
    Name = "${local.name_prefix}-billing-database"
  })
}

resource "aws_secretsmanager_secret_version" "billing_database" {
  secret_id = aws_secretsmanager_secret.billing_database.id
  secret_string = jsonencode({
    host     = module.rds.db_endpoint
    port     = 5432
    database = "basilica_v3_billing"
    username = module.rds.db_username
    password = module.rds.db_password
    url      = "postgresql://${module.rds.db_username}:${module.rds.db_password}@${module.rds.db_endpoint}/basilica_v3_billing"
  })
}
