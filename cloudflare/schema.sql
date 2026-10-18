-- Fridgeflow D1 schema. Idempotent; safe to re-run.
--
-- `accounts` holds the login identity for each user (name + salted PBKDF2
-- PIN hash). `inventories` holds the appliance/group/item data as a single
-- JSON blob per account, which mirrors the atomic-rewrite semantics of the
-- original filesystem backend and keeps queries dead simple for a tiny app.

CREATE TABLE IF NOT EXISTS accounts (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  name_lower  TEXT NOT NULL UNIQUE,
  pin_hash    TEXT NOT NULL,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS inventories (
  account_id  TEXT PRIMARY KEY,
  data        TEXT NOT NULL,
  updated_at  TEXT NOT NULL,
  FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

-- One compressed JPEG per item, stored as BLOB so we don't need R2.
-- Frontend resizes to ~800px / q=0.75 before upload (~40-80 KB each),
-- well under D1's per-row response size.
CREATE TABLE IF NOT EXISTS item_photos (
  account_id  TEXT NOT NULL,
  item_id     TEXT NOT NULL,
  data        BLOB NOT NULL,
  updated_at  TEXT NOT NULL,
  PRIMARY KEY (account_id, item_id),
  FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);
