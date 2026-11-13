-- One optional photo per menu item and pack. Images are stored locally and
-- served from public, cache-busted URLs.

ALTER TABLE items ADD COLUMN image_mime TEXT;
ALTER TABLE items ADD COLUMN image_bytes BLOB;
ALTER TABLE items ADD COLUMN image_updated_at TEXT;

ALTER TABLE packs ADD COLUMN image_mime TEXT;
ALTER TABLE packs ADD COLUMN image_bytes BLOB;
ALTER TABLE packs ADD COLUMN image_updated_at TEXT;
