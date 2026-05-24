ALTER TABLE photos ADD COLUMN shot_at        TEXT;     -- ISO8601; NULL when no EXIF
ALTER TABLE photos ADD COLUMN blur_score     REAL;
ALTER TABLE photos ADD COLUMN is_blurry      INTEGER CHECK (is_blurry IN (0,1));
ALTER TABLE photos ADD COLUMN exposure_score REAL;
ALTER TABLE photos ADD COLUMN exposure_flag  TEXT CHECK (exposure_flag IN ('normal','over','under'));
ALTER TABLE photos ADD COLUMN phash          TEXT;
ALTER TABLE photos ADD COLUMN analysis_state TEXT NOT NULL DEFAULT 'pending'
                              CHECK (analysis_state IN ('pending','done','failed'));
ALTER TABLE photos ADD COLUMN analysis_error TEXT;     -- raw error text when failed

CREATE INDEX IF NOT EXISTS idx_photos_analysis_state ON photos(analysis_state);
CREATE INDEX IF NOT EXISTS idx_photos_shot_at        ON photos(shot_at);
