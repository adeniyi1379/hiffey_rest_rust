-- Menu: categories, items, dated prices, modifiers, packs.

CREATE TABLE categories (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    active      INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    terminal_id TEXT
);

CREATE UNIQUE INDEX idx_categories_name ON categories (name COLLATE NOCASE);

CREATE TABLE items (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    category_id         TEXT NOT NULL REFERENCES categories (id),
    unit_type           TEXT NOT NULL CHECK (unit_type IN ('spoon', 'count')),
    price_per_unit_kobo INTEGER NOT NULL CHECK (price_per_unit_kobo >= 0),
    low_stock_level     INTEGER NOT NULL DEFAULT 0 CHECK (low_stock_level >= 0),
    active              INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    terminal_id         TEXT
);

CREATE UNIQUE INDEX idx_items_name ON items (name COLLATE NOCASE);
CREATE INDEX idx_items_category ON items (category_id);

-- Dated price history. Sales store the price they were sold at; this table
-- decides which price is current at any moment.
CREATE TABLE item_prices (
    id             TEXT PRIMARY KEY,
    item_id        TEXT NOT NULL REFERENCES items (id),
    price_kobo     INTEGER NOT NULL CHECK (price_kobo >= 0),
    effective_from TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    created_by     TEXT REFERENCES users (id),
    terminal_id    TEXT
);

CREATE INDEX idx_item_prices_item ON item_prices (item_id, effective_from);

CREATE TABLE modifiers (
    id               TEXT PRIMARY KEY,
    item_id          TEXT NOT NULL REFERENCES items (id),
    name             TEXT NOT NULL,
    extra_price_kobo INTEGER NOT NULL DEFAULT 0 CHECK (extra_price_kobo >= 0),
    extra_units      INTEGER NOT NULL DEFAULT 0,
    active           INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    terminal_id      TEXT
);

CREATE INDEX idx_modifiers_item ON modifiers (item_id);

CREATE TABLE packs (
    id               TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    pricing_mode     TEXT NOT NULL CHECK (pricing_mode IN ('calculated', 'fixed')),
    fixed_price_kobo INTEGER CHECK (fixed_price_kobo IS NULL OR fixed_price_kobo >= 0),
    active           INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    terminal_id      TEXT
);

CREATE UNIQUE INDEX idx_packs_name ON packs (name COLLATE NOCASE);

CREATE TABLE pack_components (
    id         TEXT PRIMARY KEY,
    pack_id    TEXT NOT NULL REFERENCES packs (id) ON DELETE CASCADE,
    item_id    TEXT NOT NULL REFERENCES items (id),
    quantity   INTEGER NOT NULL CHECK (quantity > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    terminal_id TEXT,
    UNIQUE (pack_id, item_id)
);

CREATE INDEX idx_pack_components_pack ON pack_components (pack_id);
