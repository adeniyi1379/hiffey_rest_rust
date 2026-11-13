-- Print jobs store the exact receipt text queued at the counter, so a reprint
-- or a retry after a paper jam matches what the customer was charged.

ALTER TABLE print_jobs ADD COLUMN user_id TEXT REFERENCES users (id);
ALTER TABLE print_jobs ADD COLUMN terminal_id TEXT;
ALTER TABLE print_jobs ADD COLUMN paper_width TEXT NOT NULL DEFAULT 'mm80';
ALTER TABLE print_jobs ADD COLUMN payload TEXT NOT NULL DEFAULT '';

CREATE INDEX idx_print_jobs_order ON print_jobs (order_id);
