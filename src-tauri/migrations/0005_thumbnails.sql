-- Thumbnail pre-generation state (M2).
--
-- Additive: tracks the per-photo WebP thumbnail lifecycle, mirroring the
-- analysis_state column from 0002. `thumb_src_mtime` records the source file
-- mtime (nanoseconds) the thumbnail was generated against; on the next
-- generation run a mismatch (or a missing file) triggers a self-healing regen,
-- since the scanner is INSERT OR IGNORE and never re-touches existing rows.
ALTER TABLE photos ADD COLUMN thumb_status TEXT NOT NULL DEFAULT 'pending'
                              CHECK (thumb_status IN ('pending','done','failed'));
ALTER TABLE photos ADD COLUMN thumb_src_mtime INTEGER;   -- source mtime (nanos) at gen time; NULL until generated
ALTER TABLE photos ADD COLUMN thumb_error     TEXT;      -- raw error text when failed

CREATE INDEX IF NOT EXISTS idx_photos_thumb_status ON photos(thumb_status);
