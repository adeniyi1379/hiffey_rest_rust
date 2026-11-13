-- Selling: orders, lines, payments, stock ledger and print jobs.
-- Discounts and VAT are intentionally absent from this build.

CREATE TABLE orders (
    id                 TEXT PRIMARY KEY,
    order_no           TEXT NOT NULL UNIQUE,
    trading_day        TEXT NOT NULL,
    order_seq          INTEGER NOT NULL,
    order_type         TEXT NOT NULL CHECK (order_type IN ('dine_in', 'takeaway')),
    table_no           TEXT,
    status             TEXT NOT NULL CHECK (status IN ('held', 'paid', 'voided')),
    subtotal_kobo      INTEGER NOT NULL DEFAULT 0 CHECK (subtotal_kobo >= 0),
    packaging_fee_kobo INTEGER NOT NULL DEFAULT 0 CHECK (packaging_fee_kobo >= 0),
    total_kobo         INTEGER NOT NULL DEFAULT 0 CHECK (total_kobo >= 0),
    user_id            TEXT NOT NULL REFERENCES users (id),
    terminal_id        TEXT,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    paid_at            TEXT,
    voided_at          TEXT,
    voided_by          TEXT REFERENCES users (id),
    void_reason        TEXT
);

CREATE INDEX idx_orders_day ON orders (trading_day);
CREATE INDEX idx_orders_status ON orders (status);

-- One row per trading day; the server bumps it inside the sale transaction.
CREATE TABLE order_seq (
    day      TEXT PRIMARY KEY,
    last_seq INTEGER NOT NULL CHECK (last_seq > 0)
);

CREATE TABLE order_lines (
    id              TEXT PRIMARY KEY,
    order_id        TEXT NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
    line_no         INTEGER NOT NULL,
    kind            TEXT NOT NULL CHECK (kind IN ('item', 'pack')),
    item_id         TEXT REFERENCES items (id),
    pack_id         TEXT REFERENCES packs (id),
    name            TEXT NOT NULL,
    quantity        INTEGER NOT NULL CHECK (quantity > 0),
    unit_price_kobo INTEGER NOT NULL CHECK (unit_price_kobo >= 0),
    line_total_kobo INTEGER NOT NULL CHECK (line_total_kobo >= 0),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE (order_id, line_no)
);

CREATE INDEX idx_order_lines_order ON order_lines (order_id);

-- Snapshot of every stock-bearing item in a line: the item itself for a custom
-- plate, or each component for a pack. Stored at sale time so later menu edits
-- cannot rewrite history.
CREATE TABLE order_line_components (
    id              TEXT PRIMARY KEY,
    order_line_id   TEXT NOT NULL REFERENCES order_lines (id) ON DELETE CASCADE,
    item_id         TEXT NOT NULL REFERENCES items (id),
    item_name       TEXT NOT NULL,
    unit_type       TEXT NOT NULL,
    quantity        INTEGER NOT NULL CHECK (quantity > 0),
    unit_price_kobo INTEGER NOT NULL CHECK (unit_price_kobo >= 0)
);

CREATE INDEX idx_order_line_components_line ON order_line_components (order_line_id);
CREATE INDEX idx_order_line_components_item ON order_line_components (item_id);

CREATE TABLE payments (
    id              TEXT PRIMARY KEY,
    order_id        TEXT NOT NULL REFERENCES orders (id),
    method          TEXT NOT NULL CHECK (method IN ('cash', 'transfer', 'card')),
    amount_kobo     INTEGER NOT NULL CHECK (amount_kobo > 0),
    reference       TEXT,
    confirmed_by    TEXT NOT NULL REFERENCES users (id),
    confirmed_at    TEXT NOT NULL,
    terminal_id     TEXT,
    created_at      TEXT NOT NULL
);

CREATE INDEX idx_payments_order ON payments (order_id);
CREATE INDEX idx_payments_confirmed_at ON payments (confirmed_at);

CREATE TABLE stock_ledger (
    id            TEXT PRIMARY KEY,
    item_id       TEXT NOT NULL REFERENCES items (id),
    movement_type TEXT NOT NULL CHECK (movement_type IN (
        'batch_in', 'sale', 'void_return', 'wastage', 'adjustment', 'restock', 'count_diff'
    )),
    quantity      INTEGER NOT NULL,
    ref_id        TEXT,
    reason        TEXT,
    user_id       TEXT REFERENCES users (id),
    terminal_id   TEXT,
    created_at    TEXT NOT NULL
);

CREATE INDEX idx_stock_ledger_item ON stock_ledger (item_id, created_at);
CREATE INDEX idx_stock_ledger_ref ON stock_ledger (ref_id);

CREATE TABLE stock_balances (
    item_id    TEXT PRIMARY KEY REFERENCES items (id),
    balance    INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);

CREATE TABLE stock_counts (
    id          TEXT PRIMARY KEY,
    item_id     TEXT NOT NULL REFERENCES items (id),
    counted_qty INTEGER NOT NULL,
    expected_qty INTEGER NOT NULL,
    difference  INTEGER NOT NULL,
    counted_at  TEXT NOT NULL,
    user_id     TEXT NOT NULL REFERENCES users (id),
    terminal_id TEXT
);

CREATE TABLE print_jobs (
    id         TEXT PRIMARY KEY,
    order_id   TEXT NOT NULL REFERENCES orders (id),
    job_type   TEXT NOT NULL CHECK (job_type IN ('receipt', 'duplicate', 'slip')),
    status     TEXT NOT NULL CHECK (status IN ('queued', 'printed', 'failed')),
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_print_jobs_status ON print_jobs (status);
