-- Foundation schema for the Hiffey POS.
-- Money is stored as integer kobo. Quantities are integers. Times are RFC 3339 UTC strings.

CREATE TABLE users (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('owner', 'manager', 'cashier')),
    pin_hash TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1,
    created_by TEXT REFERENCES users (id),
    pin_reset_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    terminal_id TEXT NOT NULL
);

CREATE UNIQUE INDEX users_name_unique ON users (name COLLATE NOCASE);

CREATE TABLE sessions (
    token TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users (id),
    terminal_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE INDEX sessions_user_idx ON sessions (user_id);

CREATE TABLE login_attempts (
    id TEXT PRIMARY KEY NOT NULL,
    terminal_id TEXT NOT NULL,
    success INTEGER NOT NULL,
    at TEXT NOT NULL
);

CREATE INDEX login_attempts_terminal_at_idx ON login_attempts (terminal_id, at);

CREATE TABLE settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    business_name TEXT NOT NULL DEFAULT '',
    business_address TEXT NOT NULL DEFAULT '',
    business_phone TEXT NOT NULL DEFAULT '',
    receipt_footer TEXT NOT NULL DEFAULT '',
    packaging_fee_kobo INTEGER NOT NULL DEFAULT 0,
    low_stock_default INTEGER NOT NULL DEFAULT 5,
    updated_at TEXT NOT NULL,
    updated_by TEXT
);

INSERT INTO settings (id, updated_at) VALUES (1, '');

CREATE TABLE audit_log (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT,
    terminal_id TEXT,
    action TEXT NOT NULL,
    entity TEXT,
    entity_id TEXT,
    details TEXT,
    at TEXT NOT NULL
);

CREATE INDEX audit_log_at_idx ON audit_log (at);

-- Written in the same transaction as every change. The network upload is added later.
CREATE TABLE sync_outbox (
    id TEXT PRIMARY KEY NOT NULL,
    entity TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL,
    synced_at TEXT,
    attempts INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX sync_outbox_pending_idx ON sync_outbox (synced_at);

CREATE TABLE sync_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_success_at TEXT,
    last_error TEXT,
    pending_count INTEGER NOT NULL DEFAULT 0
);

INSERT INTO sync_state (id) VALUES (1);
