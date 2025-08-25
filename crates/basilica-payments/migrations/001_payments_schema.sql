-- Payments schema for deposit account management and TAO → USD conversion
-- This schema handles per-user deposit wallets, blockchain monitoring, and credit application

CREATE TABLE IF NOT EXISTS deposit_accounts (
  user_id            TEXT PRIMARY KEY,
  address_ss58       TEXT NOT NULL UNIQUE,
  account_id_hex     TEXT NOT NULL UNIQUE,      -- 32-byte lower-hex (no 0x)
  hotkey_public_hex  TEXT NOT NULL,
  hotkey_mnemonic_ct TEXT NOT NULL,             -- encrypted ciphertext (see crypto.rs)
  created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS observed_deposits (
  block_number       BIGINT NOT NULL,
  event_index        INT NOT NULL,
  to_account_hex     TEXT NOT NULL,
  from_account_hex   TEXT NOT NULL,
  amount_plancks     NUMERIC(78,0) NOT NULL,
  status             TEXT NOT NULL,             -- FINALIZED|CREDITED
  observed_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
  credited_at        TIMESTAMPTZ,
  billing_credit_id  TEXT,
  PRIMARY KEY (block_number, event_index),
  CONSTRAINT fk_to_deposit FOREIGN KEY (to_account_hex)
    REFERENCES deposit_accounts(account_id_hex)
);

CREATE TABLE IF NOT EXISTS billing_outbox (
  id                BIGSERIAL PRIMARY KEY,
  user_id           TEXT NOT NULL,
  amount_plancks    NUMERIC(78,0) NOT NULL,
  transaction_id    TEXT NOT NULL UNIQUE,       -- "b<block>#e<idx>#<to_hex>"
  attempts          INT NOT NULL DEFAULT 0,
  claimed_at        TIMESTAMPTZ,
  next_attempt_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
  created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
  dispatched_at     TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_observed_to ON observed_deposits (to_account_hex);
CREATE INDEX IF NOT EXISTS idx_outbox_ready ON billing_outbox (dispatched_at, next_attempt_at);