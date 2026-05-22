CREATE TABLE IF NOT EXISTS photos (
  id         TEXT PRIMARY KEY,
  path       TEXT NOT NULL UNIQUE,
  status     TEXT NOT NULL DEFAULT 'pending'
             CHECK (status IN ('pending','keep','reject')),
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_photos_status ON photos(status);
