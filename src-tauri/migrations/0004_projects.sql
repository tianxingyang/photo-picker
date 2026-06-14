-- Project-based workspace isolation.
--
-- Introduces the `projects` entity and scopes every photo / group row to one
-- project. R5 (PRD) discards all pre-existing data — the dev DB held only test
-- imports — so this migration DROPs and recreates photos / similar_groups /
-- group_members rather than writing an id-recompute / data-carry path. SQLite
-- cannot ALTER a primary key or drop a column UNIQUE in place, which is the
-- other reason a rebuild (not ALTER) is used here.

CREATE TABLE projects (
  id             TEXT PRIMARY KEY,          -- UUID v4, generated in create_project
  name           TEXT NOT NULL,
  created_at     TEXT NOT NULL,             -- RFC3339 UTC
  last_opened_at TEXT                       -- NULL until first open
);

-- Drop dependents before dependencies. group_members references both
-- similar_groups and photos; similar_groups and photos are independent.
DROP TABLE group_members;
DROP TABLE similar_groups;
DROP TABLE photos;

CREATE TABLE photos (
  id             TEXT PRIMARY KEY,          -- blake3(project_id + '\n' + path)
  project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  path           TEXT NOT NULL,
  status         TEXT NOT NULL DEFAULT 'pending'
                 CHECK (status IN ('pending','keep','reject')),
  created_at     TEXT NOT NULL,
  shot_at        TEXT,                       -- ISO8601; NULL when no EXIF
  blur_score     REAL,
  is_blurry      INTEGER CHECK (is_blurry IN (0,1)),
  exposure_score REAL,
  exposure_flag  TEXT CHECK (exposure_flag IN ('normal','over','under')),
  phash          TEXT,
  analysis_state TEXT NOT NULL DEFAULT 'pending'
                 CHECK (analysis_state IN ('pending','done','failed')),
  analysis_error TEXT,                       -- raw error text when failed
  UNIQUE (project_id, path)                  -- same path may exist in another project
);

CREATE INDEX idx_photos_project        ON photos(project_id);
CREATE INDEX idx_photos_status         ON photos(status);
CREATE INDEX idx_photos_analysis_state ON photos(analysis_state);
CREATE INDEX idx_photos_shot_at        ON photos(shot_at);

CREATE TABLE similar_groups (
  id         TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  method     TEXT NOT NULL,                  -- M1 fixed 'phash_burst'
  params     TEXT NOT NULL                   -- JSON, e.g. {"threshold":8,"version":1}
);

CREATE TABLE group_members (
  group_id TEXT NOT NULL REFERENCES similar_groups(id) ON DELETE CASCADE,
  photo_id TEXT NOT NULL REFERENCES photos(id)         ON DELETE CASCADE,
  PRIMARY KEY (group_id, photo_id)
);

CREATE INDEX idx_group_members_photo    ON group_members(photo_id);
CREATE INDEX idx_similar_groups_method  ON similar_groups(method);
CREATE INDEX idx_similar_groups_project ON similar_groups(project_id);
